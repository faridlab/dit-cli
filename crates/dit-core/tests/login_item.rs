//! Open at Login for the menu bar app (ADR 0029): a LaunchAgent in a
//! LaunchAgents folder — here a tempdir, never the person's own.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use dit_core::LoginItem;

#[test]
fn turning_it_on_writes_an_agent_that_starts_the_program_and_off_removes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let item = LoginItem::in_dir(&tmp.path().join("Library/LaunchAgents"));
    assert!(!item.is_enabled());

    let program = tmp.path().join("DIT.app/Contents/MacOS/dit-tray");
    item.enable(&program).unwrap();
    assert!(item.is_enabled());
    let plist = std::fs::read_to_string(item.file()).unwrap();
    assert!(item
        .file()
        .ends_with("Library/LaunchAgents/dev.dit.tray.plist"));
    assert!(plist.contains("<string>dev.dit.tray</string>"), "{plist}");
    assert!(
        plist.contains(&format!("<string>{}</string>", program.display())),
        "{plist}"
    );
    assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"), "{plist}");

    // Turning it on twice is the same agent; off twice is still off.
    item.enable(&program).unwrap();
    item.disable().unwrap();
    assert!(!item.is_enabled());
    assert!(!item.file().exists());
    item.disable().unwrap();
}

#[test]
fn a_program_path_is_escaped_not_spliced_into_the_xml() {
    let tmp = tempfile::tempdir().unwrap();
    let item = LoginItem::in_dir(tmp.path());
    let odd = tmp.path().join("Tom & Jerry <apps>/dit-tray");
    item.enable(&odd).unwrap();
    let plist = std::fs::read_to_string(item.file()).unwrap();
    assert!(
        plist.contains("Tom &amp; Jerry &lt;apps&gt;/dit-tray"),
        "{plist}"
    );
    assert!(!plist.contains("Tom & Jerry"), "{plist}");
}
