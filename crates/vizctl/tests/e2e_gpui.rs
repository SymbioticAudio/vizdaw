//! End to end for the GPUI app: launches it with the control channel in a
//! throwaway home, plays the demo, saves, quits, relaunches and reopens.
//! Ignored by default: it needs a window server and a built app. Run it
//! with
//!
//! ```text
//! scripts/make-gpui-app.sh   # macOS: target/VizDAW-GPUI.app
//! VIZDAW_E2E_APP=target/VizDAW-GPUI.app cargo test -p vizctl --test e2e_gpui -- --ignored --nocapture
//! # Linux: VIZDAW_E2E_APP=target/debug/vizdaw-gpui (under xvfb-run -a, WAYLAND_DISPLAY unset)
//! ```
//!
//! The launch helper is copied from `e2e.rs` (only what is needed), not
//! shared. Playing is audible for a moment.

use core_control::client::Client;
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use vizctl::launch::{LaunchOptions, Launched, launch, log_tail};

const WAIT_MS: u64 = 60_000;

struct App {
    launched: Launched,
    client: Client,
}

fn app_path() -> PathBuf {
    let app =
        std::env::var("VIZDAW_E2E_APP").expect("set VIZDAW_E2E_APP to the GPUI .app or its binary");
    let p = PathBuf::from(app);
    if p.is_absolute() {
        p
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(p)
    }
}

impl App {
    fn start(home: &std::path::Path, demo: bool) -> App {
        let launched = launch(&LaunchOptions {
            app: app_path(),
            home: Some(home.to_path_buf()),
            demo,
            timeout: Duration::from_secs(180),
            env: vec![("VIZDAW_AUDIO".to_string(), "offline".to_string())],
        })
        .unwrap_or_else(|e| panic!("launch: {e}"));
        let client = Client::connect(&launched.socket).unwrap();
        App { launched, client }
    }

    fn call(&mut self, cmd: &str, params: Value) -> Value {
        let timeout = Duration::from_millis(WAIT_MS);
        self.client
            .call(cmd, params.clone(), timeout)
            .unwrap_or_else(|e| {
                panic!(
                    "{cmd} {params}: {e}\nlog:\n{}",
                    log_tail(&self.launched.log, 40)
                )
            })
    }

    fn command(&mut self, id: &str, params: Value) -> Value {
        self.call("command.run", json!({"id": id, "params": params}))
    }

    fn quit(mut self) {
        assert_eq!(
            self.call("app.quit", json!({"discard": true}))["quitting"],
            true
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Ok(Some(status)) = self.launched.child.try_wait() {
                assert!(status.success(), "the app exited with {status}");
                return;
            }
            if Instant::now() > deadline {
                let _ = self.launched.child.kill();
                panic!(
                    "the app did not quit; log:\n{}",
                    log_tail(&self.launched.log, 40)
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
#[ignore = "needs a window server and VIZDAW_E2E_APP"]
fn gpui_demo_plays_saves_and_reopens() {
    let home = tempfile::Builder::new()
        .prefix("vzg")
        .tempdir_in("/tmp")
        .unwrap();
    // The temp dir takes the process umask (0775 under Ubuntu's 002);
    // launch wants a home nobody else can write to.
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let saved = home.path().join("t.vizdaw");

    let mut app = App::start(home.path(), true);
    app.command("transport.play_stop", json!({}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    loop {
        let t = app.call("state.get", json!({"target": "transport"}));
        if t["playing"] == true {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "never started playing: {t}\nlog:\n{}",
            log_tail(&app.launched.log, 40)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    // Stopped at once: the speakers are on.
    app.command("transport.play_stop", json!({}));
    let deadline = Instant::now() + Duration::from_millis(WAIT_MS);
    while app.call("state.get", json!({"target": "transport"}))["playing"] != false {
        assert!(Instant::now() < deadline, "never stopped playing");
        std::thread::sleep(Duration::from_millis(100));
    }

    app.command("file.save_as", json!({"path": saved}));
    assert!(saved.exists(), "{} was not written", saved.display());
    app.quit();

    let mut app = App::start(home.path(), false);
    app.call("app.open", json!({"path": saved}));
    let windows = app.call("state.get", json!({"target": "windows"}));
    assert!(
        windows["windows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["title"] == "t"),
        "no window named t in {windows}"
    );
    app.quit();
}
