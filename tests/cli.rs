use std::process::Command;

#[test]
fn help_distinguishes_diagnostics_from_development_scanning_and_wia() {
    let output = Command::new(env!("CARGO_BIN_EXE_wc3119"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("wc3119 doctor"));
    assert!(text.contains("wc3119 inquiry"));
    assert!(text.contains("capture_scan"));
    assert!(text.contains("尚未提供 Windows 掃描整合"));
}

#[test]
fn rejects_unknown_and_extra_arguments_before_accessing_devices() {
    for args in [
        vec![""],
        vec!["install"],
        vec!["doctor", "--force"],
        vec!["inquiry", "--force"],
        vec!["--help", "ignored"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_wc3119"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(64));
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("不支援的參數")
        );
    }
}
