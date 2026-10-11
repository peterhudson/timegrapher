//! `timegrapher update`: is a newer release published? Asks GitHub once and
//! prints the answer with the page to download from; installs nothing.

#[cfg(feature = "update-check")]
pub fn run(json: bool) -> Result<(), String> {
    use timegrapher_core::update;
    let current = env!("CARGO_PKG_VERSION");
    let agent = format!("timegrapher/{current}");
    let check = update::check(current, &agent, std::time::Duration::from_secs(15))?;
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
