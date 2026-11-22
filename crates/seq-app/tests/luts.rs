//! LUT library (SPEC §14): files, throttled writes, rename/delete, import, apply.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lut_core::{parse_cube, write_cube, Grade, LutDoc, Table};
use resolume_client::composition::{LutEffect, LutPlace};
use seq_app::luts::{same_path, LutError, LutLibrary, SetFile};

type Sets = Arc<Mutex<Vec<(u64, String)>>>;

fn open(dir: &std::path::Path) -> (Arc<LutLibrary>, Sets) {
    let sets: Sets = Arc::default();
    let s = sets.clone();
    let set_file: SetFile = Box::new(move |id, path| s.lock().unwrap().push((id, path.to_owned())));
    (LutLibrary::open(dir, Some(set_file)), sets)
}

fn cube_on_disk(lib: &LutLibrary, name: &str) -> Table {
    let text = std::fs::read_to_string(lib.cube_path(name)).expect("cube file");
    parse_cube(&text).unwrap().1
}

#[tokio::test]
async fn create_writes_both_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, _) = open(tmp.path());
    lib.create("Warm Stage", None).unwrap();
    lib.flush().await;
    assert!(tmp.path().join("luts/Warm Stage.lut.json").is_file());
    let t = cube_on_disk(&lib, "Warm Stage");
    assert_eq!(t, Table::identity(33).pipe_round());
    let v = lib.view();
    assert_eq!(v.luts.len(), 1);
    assert!(same_path(&v.luts[0].path, &lib.cube_path("Warm Stage")));
}

trait Round {
    fn pipe_round(self) -> Table;
}
impl Round for Table {
    /// The table as written to a .cube (six decimals) and read back.
    fn pipe_round(self) -> Table {
        parse_cube(&write_cube("x", &self)).unwrap().1
    }
}

#[tokio::test]
async fn names_are_checked() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, _) = open(tmp.path());
    lib.create("Warm", None).unwrap();
    assert_eq!(
        lib.create("warm", None),
        Err(LutError::Exists("warm".into()))
    );
    assert!(matches!(lib.create("a/b", None), Err(LutError::BadName(_))));
    assert!(matches!(
        lib.create("x", Some("nope")),
        Err(LutError::NotFound(_))
    ));
    assert_eq!(lib.free_name("Warm"), "Warm 2");
    assert_eq!(lib.free_name("  "), "LUT");
    assert_eq!(lib.free_name("a/b"), "ab");
}

#[tokio::test]
async fn fast_updates_end_with_the_last_value_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, _) = open(tmp.path());
    lib.create("Drag", None).unwrap();
    let rev0 = lib.table("Drag").unwrap().0;
    for i in 0..=40 {
        let g = Grade {
            temperature: i as f32 / 40.0,
            ..Grade::default()
        };
        lib.update("Drag", g, Default::default()).unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(lib.table("Drag").unwrap().0 > rev0);
    lib.flush().await;
    let want = LutDoc {
        grade: Grade {
            temperature: 1.0,
            ..Grade::default()
        },
        ..LutDoc::default()
    }
    .bake()
    .pipe_round();
    assert_eq!(cube_on_disk(&lib, "Drag"), want);
    // Unchanged values bump nothing.
    let rev = lib.table("Drag").unwrap().0;
    lib.update(
        "Drag",
        Grade {
            temperature: 1.0,
            ..Grade::default()
        },
        Default::default(),
    )
    .unwrap();
    assert_eq!(lib.table("Drag").unwrap().0, rev);
}

#[tokio::test]
async fn a_pending_write_happens_without_a_flush() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, _) = open(tmp.path());
    lib.create("A", None).unwrap();
    lib.update(
        "A",
        Grade {
            saturation: 0.0,
            ..Grade::default()
        },
        Default::default(),
    )
    .unwrap();
    lib.update(
        "A",
        Grade {
            saturation: 0.5,
            ..Grade::default()
        },
        Default::default(),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let want = LutDoc {
        grade: Grade {
            saturation: 0.5,
            ..Grade::default()
        },
        ..LutDoc::default()
    }
    .bake()
    .pipe_round();
    assert_eq!(cube_on_disk(&lib, "A"), want);
}

fn effect(id: u64, path: &str) -> LutEffect {
    LutEffect {
        effect_id: id,
        file_param: id + 1,
        path: path.into(),
        place: LutPlace::Composition,
    }
}

#[tokio::test]
async fn rename_moves_the_files_and_repoints_the_effects() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, sets) = open(tmp.path());
    lib.create("Old", None).unwrap();
    lib.flush().await;
    let old = lib.cube_path("Old").display().to_string();
    // One effect uses it (other case and slashes), one uses another file.
    let effects = vec![
        effect(10, &old.to_uppercase().replace('\\', "/")),
        effect(20, "C:\\other.cube"),
    ];
    assert_eq!(lib.rename("Old", "New", &effects), Ok(1));
    lib.flush().await;
    let new = lib.cube_path("New").display().to_string();
    assert_eq!(*sets.lock().unwrap(), vec![(11, new)]);
    assert!(lib.cube_path("New").is_file());
    assert!(
        lib.cube_path("Old").is_file(),
        "the old file stays a moment"
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!lib.cube_path("Old").exists());
    assert!(!tmp.path().join("luts/Old.lut.json").exists());
    assert_eq!(
        lib.view()
            .luts
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>(),
        ["New"]
    );
}

#[tokio::test]
async fn delete_removes_the_files() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, _) = open(tmp.path());
    lib.create("Gone", None).unwrap();
    lib.flush().await;
    lib.delete("Gone").unwrap();
    lib.flush().await;
    assert!(!lib.cube_path("Gone").exists());
    assert!(lib.view().luts.is_empty());
    assert_eq!(lib.delete("Gone"), Err(LutError::NotFound("Gone".into())));
}

#[tokio::test]
async fn import_keeps_the_table_as_base_and_reload_restores_it() {
    let tmp = tempfile::tempdir().unwrap();
    let inv = Table::from_fn(9, |c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]]);
    {
        let (lib, _) = open(tmp.path());
        assert!(matches!(
            lib.import("Bad", "hello"),
            Err(LutError::BadCube(_))
        ));
        lib.import("Invert", &write_cube("inv", &inv)).unwrap();
        lib.create("Plain", None).unwrap();
        lib.update(
            "Plain",
            Grade {
                exposure: 1.0,
                ..Grade::default()
            },
            Default::default(),
        )
        .unwrap();
        assert!(
            lib.view()
                .luts
                .iter()
                .find(|l| l.name == "Invert")
                .unwrap()
                .has_base
        );
        lib.flush().await;
    }
    let (lib, _) = open(tmp.path());
    let v = lib.view();
    assert_eq!(
        v.luts.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(),
        ["Invert", "Plain"]
    );
    assert!(v.luts[0].has_base);
    assert_eq!(v.luts[1].grade.exposure, 1.0);
    let s = lib.table("Invert").unwrap().1.sample([0.2, 0.3, 0.4]);
    assert!((s[0] - 0.8).abs() < 1e-3 && (s[2] - 0.6).abs() < 1e-3);
    lib.clear_base("Invert").unwrap();
    assert!(!lib.view().luts[0].has_base);
}

#[tokio::test]
async fn apply_writes_the_file_then_sets_the_parameter() {
    let tmp = tempfile::tempdir().unwrap();
    let (lib, sets) = open(tmp.path());
    lib.create("Show", None).unwrap();
    lib.apply("Show", 77).await.unwrap();
    assert!(lib.cube_path("Show").is_file());
    assert_eq!(
        *sets.lock().unwrap(),
        vec![(77, lib.cube_path("Show").display().to_string())]
    );
    assert_eq!(
        lib.apply("Nope", 1).await,
        Err(LutError::NotFound("Nope".into()))
    );
    let none = LutLibrary::open(&tmp.path().join("other"), None);
    none.create("X", None).unwrap();
    assert_eq!(none.apply("X", 1).await, Err(LutError::NoResolume));
}

#[tokio::test]
async fn an_unchanged_cube_is_not_rewritten_on_start() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let (lib, _) = open(tmp.path());
        lib.create("Keep", None).unwrap();
        lib.flush().await;
    }
    let path = tmp.path().join("luts/Keep.cube");
    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let (lib, _) = open(tmp.path());
    lib.flush().await;
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before
    );
}
