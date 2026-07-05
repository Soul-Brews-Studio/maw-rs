fn write_pane_title(
    runner: &mut dyn TmuxRunner,
    target: &str,
    title: Option<&str>,
) -> Result<(), TmuxError> {
    let Some(title) = title else {
        return Ok(());
    };
    runner.run(
        "select-pane",
        &[
            "-t".to_owned(),
            target.to_owned(),
            "-T".to_owned(),
            title.to_owned(),
        ],
    )?;
    Ok(())
}
