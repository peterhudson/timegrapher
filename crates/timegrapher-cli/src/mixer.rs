//! What to change on the computer to fix a microphone problem.
//!
//! On Linux we read the ALSA mixer with `amixer` and propose exact
//! commands; on macOS and Windows we say which setting to change. Nothing
//! here changes a setting unless `apply` is called, and the CLI only calls
//! it when the person passed `--apply`.

use serde::Serialize;
use std::process::Command;
use timegrapher_core::diagnose::{IssueCode, SignalCheck};

/// One control from `amixer scontents`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Control {
    pub name: String,
    pub index: u32,
    /// Capture volume steps: current, min, max.
    pub capture: Option<(i64, i64, i64)>,
    /// Current capture gain in dB, when the driver reports it.
    pub capture_db: Option<f64>,
    /// On/off switch state, when the control has one.
    pub switch: Option<bool>,
}

/// The mixer of one sound card.
#[derive(Debug, Clone, Serialize)]
pub struct Mixer {
    pub card: String,
    pub controls: Vec<Control>,
}

impl Mixer {
    /// Automatic gain control, if the card has one.
    pub fn agc(&self) -> Option<&Control> {
        self.controls
            .iter()
            .find(|c| c.switch.is_some() && is_agc(&c.name))
    }

    /// The capture level control: `Mic` first, then `Capture`, then any.
    pub fn capture_level(&self) -> Option<&Control> {
        let vol = || self.controls.iter().filter(|c| c.capture.is_some());
        vol()
            .find(|c| c.name == "Mic")
            .or_else(|| vol().find(|c| c.name == "Capture"))
            .or_else(|| vol().next())
    }
}

fn is_agc(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("auto gain") || n.contains("agc")
}

/// A proposed change.
#[derive(Debug, Clone, Serialize)]
pub struct Fix {
    pub issue: IssueCode,
    pub description: String,
    /// The command that makes the change, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Always true: ask the person before changing their computer.
    pub requires_consent: bool,
}

/// Parse the output of `amixer -c CARD scontents`.
pub fn parse_scontents(card: &str, text: &str) -> Mixer {
    let mut controls: Vec<Control> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("Simple mixer control '") {
            let (name, idx) = rest.rsplit_once("',").unwrap_or((rest, "0"));
            controls.push(Control {
                name: name.to_string(),
                index: idx.trim().parse().unwrap_or(0),
                capture: None,
                capture_db: None,
                switch: None,
            });
            continue;
        }
        let Some(c) = controls.last_mut() else {
            continue;
        };
        let l = line.trim();
        if let Some(lim) = l.strip_prefix("Limits:") {
            if let Some(after) = lim.split("Capture").nth(1) {
                let nums: Vec<i64> = after
                    .split(|ch: char| !(ch.is_ascii_digit() || ch == '-'))
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if let [lo, hi, ..] = nums[..] {
                    c.capture = Some((lo, lo, hi));
                }
            }
        } else if l.contains(':') && !l.starts_with("Capabilities") && !l.contains("channels") {
            let (_, value) = l.split_once(':').unwrap_or(("", l));
            let value = value.trim();
            // A line can carry playback and capture state; prefer capture.
            let part = value.find("Capture ").map_or(value, |p| &value[p..]);
            if let Some(v) = part.strip_prefix("Capture ") {
                if let (Some((_, lo, hi)), Some(cur)) = (
                    c.capture,
                    v.split_whitespace().next().and_then(|s| s.parse().ok()),
                ) {
                    c.capture = Some((cur, lo, hi));
                }
            }
            for item in part.split('[').skip(1) {
                let item = item.trim_end_matches([']', ' ']);
                match item {
                    "on" => c.switch = Some(true),
                    "off" => c.switch = Some(false),
                    p if p.ends_with("dB") && part.starts_with("Capture") => {
                        c.capture_db = p.trim_end_matches("dB").parse().ok()
                    }
                    _ => {}
                }
            }
        }
    }
    Mixer {
        card: card.to_string(),
        controls,
    }
}

/// Read the ALSA mixer of `card` (a number or name), or `None` when
/// `amixer` is missing or the card unknown.
pub fn read_alsa(card: &str) -> Option<Mixer> {
    let out = Command::new("amixer")
        .args(["-c", card, "scontents"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| parse_scontents(card, &String::from_utf8_lossy(&out.stdout)))
}

/// The ALSA card of a device id or name such as `hw:CARD=Device,DEV=0`
/// or `hw:2,0`.
pub fn card_from_device(id: &str) -> Option<String> {
    if let Some(rest) = id.split("CARD=").nth(1) {
        return Some(rest.split([',', ' ', ')']).next()?.to_string());
    }
    let rest = id.split("hw:").nth(1)?;
    let n = rest.split([',', ' ']).next()?;
    n.parse::<u32>().ok().map(|n| n.to_string())
}

/// Sound cards listed in `/proc/asound/cards`: (number, id, description).
pub fn alsa_cards() -> Vec<(u32, String, String)> {
    let Ok(text) = std::fs::read_to_string("/proc/asound/cards") else {
        return Vec::new();
    };
    let mut cards = Vec::new();
    for line in text.lines() {
        let l = line.trim_start();
        let Some((num, rest)) = l.split_once(' ') else {
            continue;
        };
        let (Ok(n), Some(open)) = (num.parse::<u32>(), rest.find('[')) else {
            continue;
        };
        let Some(close) = rest.find(']') else {
            continue;
        };
        let id = rest[open + 1..close].trim().to_string();
        let desc = rest[close + 1..].trim_start_matches([':', ' ']).to_string();
        cards.push((n, id, desc));
    }
    cards
}

/// The one USB audio card, when there is exactly one: usually the
/// timegrapher microphone.
pub fn single_usb_card() -> Option<String> {
    let usb: Vec<_> = alsa_cards()
        .into_iter()
        .filter(|c| c.2.contains("USB-Audio"))
        .collect();
    (usb.len() == 1).then(|| usb[0].0.to_string())
}

fn amixer(card: &str, control: &Control, value: &str) -> Vec<String> {
    let name = if control.index == 0 {
        control.name.clone()
    } else {
        format!("{},{}", control.name, control.index)
    };
    ["amixer", "-c", card, "sset", &name, value]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// Linux fixes from the mixer state and the signal check.
pub fn linux_fixes(check: &SignalCheck, mixer: Option<&Mixer>) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let mut fix = |issue, description: String, command| {
        fixes.push(Fix {
            issue,
            description,
            command,
            requires_consent: true,
        })
    };
    let agc = mixer.and_then(|m| m.agc().map(|c| (m, c)));
    // Automatic gain spoils every level reading, so it goes first even
    // when the signal check could not see it pumping.
    if let Some((m, c)) = agc.filter(|(_, c)| c.switch == Some(true)) {
        fix(
            IssueCode::AgcSuspected,
            format!(
                "Turn off '{}' on card {} so the gain stays fixed, then run doctor again.",
                c.name, m.card
            ),
            Some(amixer(&m.card, c, "off")),
        );
        // Levels measured while automatic gain acts mean little.
        return fixes;
    } else if check.has(IssueCode::AgcSuspected) {
        fix(
            IssueCode::AgcSuspected,
            "Automatic gain looks to be on, but the mixer shows no switch for it. Look for \
             'Auto Gain Control' in alsamixer (F4 for capture controls) or in your desktop's \
             sound settings."
                .into(),
            None,
        );
    }
    let Some(change) = check.suggested_gain_change_db else {
        return fixes;
    };
    let issue = if check.has(IssueCode::Clipping) {
        IssueCode::Clipping
    } else if check.has(IssueCode::TooQuiet) {
        IssueCode::TooQuiet
    } else {
        IssueCode::Hot
    };
    let level = mixer.and_then(|m| m.capture_level().map(|c| (m, c)));
    match level {
        Some((m, c)) => {
            let (cur, lo, hi) = c.capture.unwrap_or((0, 0, 0));
            // Without dB figures, step by a quarter of the range per 6 dB.
            let step_db = match c.capture_db {
                Some(_) => None,
                None => Some(6.0 / ((hi - lo) as f64 / 4.0).max(1.0)),
            };
            let (value, to) = match step_db {
                None => {
                    let v = format!(
                        "{:.1}dB{}",
                        change.abs(),
                        if change < 0.0 { "-" } else { "+" }
                    );
                    (v, format!("by {:+.1} dB", change))
                }
                Some(per_step) => {
                    let steps = (change / per_step).round() as i64;
                    let new = (cur + steps).clamp(lo, hi);
                    (new.to_string(), format!("from {cur} to {new} of {hi}"))
                }
            };
            fix(
                issue,
                format!(
                    "Change '{}' on card {} {to}, then run doctor again.",
                    c.name, m.card
                ),
                Some(amixer(&m.card, c, &value)),
            );
        }
        None => fix(
            issue,
            format!(
                "Change the microphone's input level by about {change:+.0} dB in your sound \
                 settings (or alsamixer, F4 for capture), then run doctor again."
            ),
            None,
        ),
    }
    fixes
}

/// Fixes on macOS and Windows: which setting to change, in words.
pub fn other_fixes(check: &SignalCheck, os: &str) -> Vec<Fix> {
    let mut fixes = Vec::new();
    let mut fix = |issue, description: &str| {
        fixes.push(Fix {
            issue,
            description: description.to_string(),
            command: None,
            requires_consent: true,
        })
    };
    let level = |down: bool| {
        match (os, down) {
        ("macos", true) => "Open Audio MIDI Setup, select the microphone, and lower its Input volume slider; or in System Settings > Sound > Input lower the Input volume. Then run doctor again.",
        ("macos", false) => "Open Audio MIDI Setup, select the microphone, and raise its Input volume slider; or in System Settings > Sound > Input raise the Input volume. Then run doctor again.",
        ("windows", true) => "Open Settings > System > Sound, choose the microphone under Input, and lower its Input volume (or Control Panel > Sound > Recording > Properties > Levels). Then run doctor again.",
        ("windows", false) => "Open Settings > System > Sound, choose the microphone under Input, and raise its Input volume (or Control Panel > Sound > Recording > Properties > Levels). Then run doctor again.",
        (_, true) => "Lower the microphone's input level in your sound settings, then run doctor again.",
        (_, false) => "Raise the microphone's input level in your sound settings, then run doctor again.",
    }
    };
    if check.has(IssueCode::AgcSuspected) {
        fix(
            IssueCode::AgcSuspected,
            match os {
                "windows" => "Turn off automatic gain: Control Panel > Sound > Recording > the microphone > Properties: untick 'Allow applications to take exclusive control' only if needed, and under Advanced or Enhancements turn off 'Audio enhancements' and any AGC option. Then run doctor again.",
                "macos" => "macOS has no system automatic gain for USB microphones, but some apps and voice-processing tools add it; quit other audio apps and run doctor again.",
                _ => "Turn off the microphone's automatic gain in your sound settings, then run doctor again.",
            },
        );
    }
    if check.has(IssueCode::Clipping) {
        fix(IssueCode::Clipping, level(true));
    } else if check.has(IssueCode::Hot) {
        fix(IssueCode::Hot, level(true));
    } else if check.has(IssueCode::TooQuiet) {
        fix(IssueCode::TooQuiet, level(false));
    }
    fixes
}

/// Run each fix's command. Only called after the person agreed.
pub fn apply(fixes: &[Fix]) -> Result<(), String> {
    for f in fixes {
        let Some(cmd) = &f.command else { continue };
        eprintln!("running: {}", shell_words(cmd));
        let st = Command::new(&cmd[0])
            .args(&cmd[1..])
            .status()
            .map_err(|e| format!("{}: {e}", cmd[0]))?;
        if !st.success() {
            return Err(format!("{} failed ({st})", shell_words(cmd)));
        }
    }
    Ok(())
}

/// A command as you would type it.
pub fn shell_words(cmd: &[String]) -> String {
    cmd.iter()
        .map(|s| {
            if s.contains(|c: char| c.is_whitespace() || c == '\'') {
                format!("'{}'", s.replace('\'', r"'\''"))
            } else {
                s.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The C-Media USB microphone as it arrived: AGC on, Mic at 16/16.
    const CMEDIA: &str = "\
Simple mixer control 'Speaker',0
  Capabilities: pvolume pswitch pswitch-joined
  Playback channels: Front Left - Front Right
  Limits: Playback 0 - 151
  Mono:
  Front Left: Playback 100 [66%] [-17.00dB] [on]
  Front Right: Playback 100 [66%] [-17.00dB] [on]
Simple mixer control 'Mic',0
  Capabilities: pvolume pvolume-joined cvolume cvolume-joined pswitch pswitch-joined cswitch cswitch-joined
  Playback channels: Mono
  Capture channels: Mono
  Limits: Playback 0 - 127 Capture 0 - 16
  Mono: Playback 0 [0%] [-23.00dB] [off] Capture 16 [100%] [23.81dB] [on]
Simple mixer control 'Auto Gain Control',0
  Capabilities: pswitch pswitch-joined
  Playback channels: Mono
  Mono: Playback [on]
";

    #[test]
    fn parses_cmedia_mixer() {
        let m = parse_scontents("2", CMEDIA);
        assert_eq!(m.controls.len(), 3);
        let agc = m.agc().unwrap();
        assert_eq!(agc.name, "Auto Gain Control");
        assert_eq!(agc.switch, Some(true));
        let mic = m.capture_level().unwrap();
        assert_eq!(mic.name, "Mic");
        assert_eq!(mic.capture, Some((16, 0, 16)));
        assert_eq!(mic.capture_db, Some(23.81));
    }

    #[test]
    fn cards_from_device_names() {
        assert_eq!(
            card_from_device("hw:CARD=Device,DEV=0").as_deref(),
            Some("Device")
        );
        assert_eq!(card_from_device("alsa:hw:2,0").as_deref(), Some("2"));
        assert_eq!(card_from_device("default"), None);
    }

    fn clipped() -> SignalCheck {
        use timegrapher_core::diagnose::{check, DiagnoseConfig};
        use timegrapher_core::synth::{generate, SynthConfig};
        let cfg = SynthConfig {
            duration_s: 4.0,
            ..Default::default()
        };
        let mut a = generate(&cfg, |_| 270.0, |_| 0.0);
        for v in a.samples.iter_mut() {
            *v = (*v * 6.0).clamp(-1.0, 1.0);
        }
        check(&a, &DiagnoseConfig::default())
    }

    #[test]
    fn agc_goes_off_first() {
        let c = clipped();
        let fixes = linux_fixes(&c, Some(&parse_scontents("2", CMEDIA)));
        assert_eq!(fixes.len(), 1);
        assert_eq!(
            shell_words(fixes[0].command.as_ref().unwrap()),
            "amixer -c 2 sset 'Auto Gain Control' off"
        );
    }

    #[test]
    fn clipping_turns_the_mic_down() {
        let c = clipped();
        let m = parse_scontents("2", &CMEDIA.replace("Playback [on]", "Playback [off]"));
        let fixes = linux_fixes(&c, Some(&m));
        assert_eq!(fixes.len(), 1);
        assert_eq!(fixes[0].issue, IssueCode::Clipping);
        assert_eq!(
            shell_words(fixes[0].command.as_ref().unwrap()),
            "amixer -c 2 sset Mic 6.0dB-"
        );
        // Without dB figures it steps: a quarter of 16 steps per 6 dB.
        let m = parse_scontents("2", &m_without_db());
        let fixes = linux_fixes(&c, Some(&m));
        assert_eq!(
            shell_words(fixes[0].command.as_ref().unwrap()),
            "amixer -c 2 sset Mic 12"
        );
    }

    fn m_without_db() -> String {
        CMEDIA
            .replace("Playback [on]", "Playback [off]")
            .replace(" [23.81dB]", "")
    }

    #[test]
    fn quoting() {
        let c: Vec<String> = ["amixer", "-c", "2", "sset", "Auto Gain Control", "off"]
            .map(String::from)
            .into();
        assert_eq!(shell_words(&c), "amixer -c 2 sset 'Auto Gain Control' off");
    }
}
