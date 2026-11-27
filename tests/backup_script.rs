use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Run {
    out: std::process::Output,
    stored: bool,
    restic_calls: String,
}

fn run_backup(tag: &str, dump_body: &str, repo_exists: bool) -> Run {
    run_backup_with(tag, "mariadb-dump", dump_body, repo_exists)
}

fn run_backup_with(tag: &str, dump_name: &str, dump_body: &str, repo_exists: bool) -> Run {
    let dir = std::env::temp_dir().join(format!("pgw-backup-script-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let stored = dir.join("stored.sql");
    let calls = dir.join("calls.log");
    script(&dir, dump_name, dump_body);
    let cat_config = if repo_exists { "exit 0" } else { "exit 1" };
    script(
        &dir,
        "restic",
        &format!(
            "echo \"$@\" >> {calls}\ncase \"$1\" in\n  backup) cat > {stored} ;;\n  cat) {cat_config} ;;\n  *) exit 0 ;;\nesac",
            calls = calls.display(),
            stored = stored.display(),
        ),
    );
    let path = format!("{}:{}", dir.display(), std::env::var("PATH").unwrap());
    let out = Command::new(format!("{}/scripts/backup.sh", env!("CARGO_MANIFEST_DIR")))
        .env("PATH", path)
        .env("DUMP_BIN", dir.join(dump_name))
        .env("DB_NAME", "gateway")
        .env("RESTIC_REPOSITORY", "s3:example/none")
        .env("RESTIC_PASSWORD", "x")
        .env("RESTIC_FORGET_ARGS", "--keep-daily 14 --keep-weekly 8")
        .output()
        .unwrap();
    let run = Run {
        out,
        stored: stored.exists(),
        restic_calls: fs::read_to_string(&calls).unwrap_or_default(),
    };
    let _ = fs::remove_dir_all(&dir);
    run
}

const GOOD_DUMP: &str = "echo 'create table t (a int);'; echo '-- Dump completed on now'";

#[test]
fn restic_backup_stores_a_complete_dump() {
    let run = run_backup("good", GOOD_DUMP, true);
    assert!(
        run.out.status.success(),
        "{}",
        String::from_utf8_lossy(&run.out.stderr)
    );
    assert!(run.stored);
}

#[test]
fn restic_backup_never_stores_a_truncated_dump() {
    let run = run_backup("truncated", "echo 'create table t (a int);'; exit 2", true);
    assert!(!run.out.status.success());
    assert!(!run.stored, "a partial dump was handed to restic");
}

#[test]
fn restic_backup_initialises_a_missing_repository() {
    let run = run_backup("init", GOOD_DUMP, false);
    assert!(run.out.status.success());
    assert!(run.restic_calls.lines().any(|l| l == "init"));
    let existing = run_backup("noinit", GOOD_DUMP, true);
    assert!(!existing.restic_calls.lines().any(|l| l == "init"));
}

#[test]
fn restic_backup_applies_the_retention_policy_after_the_snapshot() {
    let run = run_backup("forget", GOOD_DUMP, true);
    let calls: Vec<&str> = run.restic_calls.lines().collect();
    let backup = calls.iter().position(|l| l.starts_with("backup")).unwrap();
    let forget = calls.iter().position(|l| l.starts_with("forget")).unwrap();
    assert!(backup < forget);
    assert!(calls[forget].contains("--keep-daily 14 --keep-weekly 8"));
    assert!(calls[forget].contains("--tag payment-gateway"));
    assert!(calls[backup].contains("--host payment-gateway"));
    assert!(calls[forget].contains("--host payment-gateway"));
}

const MYSQL_DUMP: &str = "case \"$1\" in --help) echo '  --column-statistics Add an ANALYZE TABLE statement'; exit 0 ;; esac
case \" $* \" in *' --column-statistics=0 '*) ;; *) echo \"Unknown table 'COLUMN_STATISTICS' in information_schema (1109)\" >&2; exit 2 ;; esac
echo 'create table t (a int);'; echo '-- Dump completed on now'";

const MARIADB_DUMP: &str = "case \"$1\" in --help) echo '  --single-transaction'; exit 0 ;; esac
case \" $* \" in *' --column-statistics'*) echo \"unknown variable 'column-statistics=0'\" >&2; exit 7 ;; esac
echo 'create table t (a int);'; echo '-- Dump completed on now'";

#[test]
fn mysql_client_dump_of_a_mariadb_server_skips_column_statistics() {
    let run = run_backup_with("mysqldump", "mysqldump", MYSQL_DUMP, true);
    assert!(
        run.out.status.success(),
        "{}",
        String::from_utf8_lossy(&run.out.stderr)
    );
    assert!(run.stored);
}

#[test]
fn mariadb_dump_is_not_given_the_mysql_only_flag() {
    let run = run_backup_with("mariadbdump", "mariadb-dump", MARIADB_DUMP, true);
    assert!(
        run.out.status.success(),
        "{}",
        String::from_utf8_lossy(&run.out.stderr)
    );
    assert!(run.stored);
}
