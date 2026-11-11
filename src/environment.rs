//! Stable measurement identity and separately recorded transient conditions.
use serde_json::{json, Value};
use std::{path::Path, process::Command, time::Instant};

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}
fn command(program: &str, args: &[&str]) -> Option<String> {
    crate::process::text(Command::new(program).args(args))
}
fn filesystem(path: &Path) -> Value {
    let mut existing = path;
    while !existing.exists() {
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent;
    }
    let path = existing;
    let mount = command(
        "findmnt",
        &[
            "--json",
            "--target",
            &path.to_string_lossy(),
            "--output",
            "FSTYPE,SOURCE,OPTIONS,MAJ:MIN",
        ],
    )
    .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    .and_then(|v| v["filesystems"].as_array().and_then(|a| a.first()).cloned());
    let Some(mut mount) = mount else {
        return Value::Null;
    };
    if let Some(id) = mount["maj:min"].as_str() {
        if let Ok(mut device) = std::fs::canonicalize(format!("/sys/dev/block/{id}")) {
            if device.join("partition").exists() {
                device.pop();
            }
            mount["device"] = json!({"model":read(device.join("device/model")),
                "scheduler":read(device.join("queue/scheduler")),"rotational":read(device.join("queue/rotational")),
                "read_ahead_kb":read(device.join("queue/read_ahead_kb"))});
        }
    }
    mount
}

fn power_supplies() -> Value {
    let power: std::collections::BTreeMap<_, _> = std::fs::read_dir("/sys/class/power_supply")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| {
            (
                entry.file_name().to_string_lossy().into_owned(),
                json!({"type":read(entry.path().join("type")),
            "online":read(entry.path().join("online")),"status":read(entry.path().join("status"))}),
            )
        })
        .collect();
    json!(power)
}

fn power_identity(mut power: Value) -> Value {
    // Charge completion/threshold cycling is transient when external power is
    // positively observed. Unknown supplies and battery discharge stay distinct.
    let external_online = power.as_object().is_some_and(|supplies| {
        supplies
            .values()
            .any(|s| s["type"].as_str().is_some_and(|kind| kind != "Battery") && s["online"] == "1")
    });
    if external_online {
        if let Some(supplies) = power.as_object_mut() {
            for supply in supplies.values_mut() {
                if supply["type"] == "Battery"
                    && matches!(
                        supply["status"].as_str(),
                        Some("Charging" | "Full" | "Not charging")
                    )
                {
                    supply["status"] = json!("not_discharging_on_external_power");
                }
            }
        }
    }
    power
}

pub fn identity(root: &Path) -> Value {
    let governors: std::collections::BTreeMap<_, _> = std::fs::read_dir("/sys/devices/system/cpu/cpufreq").into_iter().flatten().filter_map(Result::ok)
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(),json!({
            "governor":read(entry.path().join("scaling_governor")),"driver":read(entry.path().join("scaling_driver")),
            "min_khz":read(entry.path().join("scaling_min_freq")),"max_khz":read(entry.path().join("scaling_max_freq"))}))).collect();
    let power = power_identity(power_supplies());
    json!({"project_filesystem":filesystem(root),"temp_filesystem":filesystem(&std::env::temp_dir()),
        "target_filesystem":filesystem(&crate::shared::cargo_target_directory(root)),
        "governors":governors,"power_supplies":power,"power_profile":command("powerprofilesctl",&["get"]),
        "kernel":command("uname",&["-r"]),"control_revision":3,"minimum_independent_launches":std::env::var("SPLENS_MIN_INDEPENDENT_LAUNCHES").unwrap_or_else(|_| "1".into()),"power_identity_revision":2})
}

pub fn conditions() -> Value {
    let temperatures: std::collections::BTreeMap<_, _> = std::fs::read_dir("/sys/class/thermal")
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("thermal_zone"))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                read(e.path().join("temp")),
            )
        })
        .collect();
    json!({"loadavg":read("/proc/loadavg"),"cpu_pressure":read("/proc/pressure/cpu"),
        "io_pressure":read("/proc/pressure/io"),"memory_pressure":read("/proc/pressure/memory"),
        "temperatures_millicelsius":temperatures,"power_supplies":power_supplies()})
}

pub fn control() -> Value {
    let bytes: Vec<_> = (0..256 * 1024).map(|i| (i % 251) as u8).collect();
    // Warm the fixed control itself before sampling; operation timings are untouched.
    let warmup = Instant::now();
    let mut warmup_iterations = 0_u64;
    while warmup.elapsed() < std::time::Duration::from_millis(100) {
        std::hint::black_box(crate::provenance::fingerprint(&bytes));
        warmup_iterations += 1;
    }
    let warmup_ns = warmup.elapsed().as_nanos() as u64;
    let mut samples = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..16 {
            std::hint::black_box(crate::provenance::fingerprint(std::hint::black_box(&bytes)));
        }
        samples.push((start.elapsed().as_nanos() / 16) as u64);
    }
    samples.sort_unstable();
    json!({"revision":3,"iterations_per_sample":16,"warmup_ns":warmup_ns,"warmup_iterations":warmup_iterations,"metric":"fixed_256_kib_hash_ns","median_ns":samples[3],"min_ns":samples[0],"max_ns":samples[6],"samples_ns":samples})
}

pub fn quality(before: &Value, after: &Value) -> Value {
    let valid = |control: &Value| {
        let (Some(min), Some(median), Some(max)) = (
            control["min_ns"].as_f64(),
            control["median_ns"].as_f64(),
            control["max_ns"].as_f64(),
        ) else {
            return false;
        };
        min > 0.0 && min <= median && median <= max && max.is_finite()
    };
    let pair = (valid(before) && valid(after)).then(|| {
        (
            before["median_ns"].as_f64().unwrap(),
            after["median_ns"].as_f64().unwrap(),
        )
    });
    let ratio = pair.map(|(a, b)| a.max(b) / a.min(b));
    let dispersion = pair.map(|_| {
        [before, after]
            .into_iter()
            .map(|v| v["max_ns"].as_f64().unwrap() / v["min_ns"].as_f64().unwrap())
            .fold(1.0, f64::max)
    });
    json!({"status":if ratio.is_none() {"unknown"} else if ratio.is_some_and(|r|r > 1.25) || dispersion.is_some_and(|r|r > 1.5) {"noisy"} else {"stable"},
        "control_median_ratio":ratio,"control_max_min_ratio":dispersion,"before":before,"after":after})
}

#[cfg(test)]
mod tests {
    use super::quality;
    use serde_json::{json, Value};
    #[test]
    fn charge_completion_is_transient_but_power_source_and_discharge_changes_are_not() {
        let supplies = |online, status| {
            json!({"AC":{"type":"Mains","online":online,"status":null},
            "BAT0":{"type":"Battery","online":null,"status":status}})
        };
        let charging = super::power_identity(supplies("1", "Charging"));
        for status in ["Full", "Not charging"] {
            assert_eq!(charging, super::power_identity(supplies("1", status)));
        }
        assert_ne!(
            charging,
            super::power_identity(supplies("0", "Discharging"))
        );
        assert_ne!(
            charging,
            super::power_identity(supplies("1", "Discharging"))
        );
        assert_ne!(charging, super::power_identity(supplies("1", "Unknown")));
        assert_ne!(
            super::power_identity(supplies("0", "Charging")),
            super::power_identity(supplies("0", "Not charging"))
        );
        assert_eq!(super::power_identity(Value::Null), Value::Null);
    }

    #[test]
    fn control_drift_flags_noise_without_changing_machine_identity() {
        let control = |n| json!({"median_ns":n,"min_ns":n,"max_ns":n});
        assert_eq!(quality(&control(100), &control(130))["status"], "noisy");
        assert_eq!(quality(&control(100), &control(110))["status"], "stable");
        assert_eq!(quality(&Value::Null, &control(100))["status"], "unknown");
        for invalid in [
            json!({"median_ns":100}),
            json!({"min_ns":200,"median_ns":100,"max_ns":300}),
            json!({"min_ns":0,"median_ns":100,"max_ns":100}),
        ] {
            assert_eq!(quality(&invalid, &control(100))["status"], "unknown");
        }
    }
}
