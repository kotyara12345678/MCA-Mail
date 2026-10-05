use super::*;

const TEST_URL: &str = "postgres://mca:mca@localhost:55432/mca_mail";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

fn pool() -> PgPool {
    PgPool::connect_lazy(TEST_URL).expect("pool")
}

#[test]
fn a_disabled_service_starts_no_task() {
    let settings = BackupSettings {
        enabled: false,
        ..BackupSettings::default()
    };
    runtime().block_on(async {
        let (_tx, rx) = watch::channel(false);
        assert!(spawn(&settings, TEST_URL, pool(), rx).is_none());
    });
}

#[test]
fn an_enabled_service_starts_a_task() {
    let settings = BackupSettings {
        enabled: true,
        dir: std::env::temp_dir().join("mca-backup-enabled-test"),
        run_on_startup: false,
        ..BackupSettings::default()
    };
    runtime().block_on(async {
        let (_tx, rx) = watch::channel(false);
        let handle = spawn(&settings, TEST_URL, pool(), rx).expect("worker");
        handle.abort();
    });
}

#[test]
fn a_broken_database_url_stops_backups_without_panicking() {
    let settings = BackupSettings {
        enabled: true,
        ..BackupSettings::default()
    };
    runtime().block_on(async {
        let (_tx, rx) = watch::channel(false);
        assert!(spawn(&settings, "not a url", pool(), rx).is_none());
    });
}
