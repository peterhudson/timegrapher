//! `timegrapher update`: is a newer release published? Asks GitHub once and
//! prints the answer with the page to download from; installs nothing.

#[cfg(feature = "update-check")]
pub fn run(json: bool) -> Result<(), String> {
    use timegrapher_core::update;
    let current = env!("CARGO_PKG_VERSION");
    let agent = format!("timegrapher/{current}");
    let Some(check) = update::check_published(current, &agent, std::time::Duration::from_secs(15))?
    else {
        // Nothing published yet: there is nothing newer, so this is an
        // answer, not an error.
        if json {
            return crate::output::print(
                "update",
                serde_json::json!({ "source": update::LATEST_RELEASE_API }),
                &serde_json::json!({
                    "current": current,
                    "latest": null,
                    "update_available": false,
                }),
            );
        }
        println!("Timegrapher {current} is up to date: no release has been published yet.");
        println!("Releases will appear at {}", update::RELEASES_PAGE);
        return Ok(());
    };
    if json {
        return crate::output::print(
            "update",
            serde_json::json!({ "source": update::LATEST_RELEASE_API }),
            &check,
        );
    }
    if check.update_available {
        println!(
            "Timegrapher {} is available (this is {}).",
            check.latest.version, check.current
        );
        println!("Download it from {}", check.latest.url);
    } else {
        println!(
            "Timegrapher {} is up to date (latest release {}).",
            check.current, check.latest.version
        );
    }
    Ok(())
}

#[cfg(not(feature = "update-check"))]
pub fn run(_json: bool) -> Result<(), String> {
    Err(format!(
        "this build cannot check for updates; see {}",
        timegrapher_core::update::RELEASES_PAGE
    ))
}
