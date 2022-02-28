#[test]
fn from_probing_cwd() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::File::create(dir.path().join("config")).unwrap();
    let ctx = git_worktree::fs::Context::probe(dir.path());
    dbg!(ctx);
    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str() != Some("config"))
        .map(|e| e.path().to_owned())
        .collect();
    assert_eq!(
        entries.len(),
        0,
        "there should be no left-over files after probing, found {:?}",
        entries
    );
}