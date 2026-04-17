use std::fs;
use std::path::Path;

use caravan::plan::{build_plan, load_batch_definition, plan_batches, PlanOptions};
use caravan::scan::scan_source;
use tempfile::TempDir;

fn create_file(root: &Path, rel: &str, size: usize) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent directories should be created");
    }
    fs::write(path, vec![b'x'; size]).expect("file should be created");
}

#[test]
fn empty_source_tree_produces_zero_batches() {
    let tmp = TempDir::new().expect("temp dir");
    let plan = build_plan(
        tmp.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");

    assert_eq!(plan.source_file_count, 0);
    assert!(plan.batches.is_empty());
}

#[test]
fn single_large_file_forms_one_batch() {
    let tmp = TempDir::new().expect("temp dir");
    create_file(tmp.path(), "video.bin", 2048);

    let plan = build_plan(
        tmp.path(),
        &PlanOptions {
            batch_size_bytes: 1024,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");

    assert_eq!(plan.batches.len(), 1);
    assert_eq!(plan.batches[0].file_count, 1);
    assert_eq!(plan.batches[0].total_bytes, 2048);
}

#[test]
fn many_small_files_are_split_by_file_count_and_size() {
    let tmp = TempDir::new().expect("temp dir");
    for idx in 0..10 {
        create_file(tmp.path(), &format!("docs/f{idx}.txt"), 10);
    }

    let scanned = scan_source(tmp.path()).expect("scan should succeed");
    let batches = plan_batches(
        scanned,
        &PlanOptions {
            batch_size_bytes: 100,
            max_files: Some(3),
        },
    )
    .expect("planning should succeed");

    assert_eq!(batches.len(), 4);
    assert_eq!(batches[0].file_count, 3);
    assert_eq!(batches[1].file_count, 3);
    assert_eq!(batches[2].file_count, 3);
    assert_eq!(batches[3].file_count, 1);
}

#[test]
fn directory_local_files_stay_together_when_possible() {
    let tmp = TempDir::new().expect("temp dir");
    create_file(tmp.path(), "a/1.txt", 10);
    create_file(tmp.path(), "a/2.txt", 10);
    create_file(tmp.path(), "b/1.txt", 10);
    create_file(tmp.path(), "b/2.txt", 10);

    let scanned = scan_source(tmp.path()).expect("scan should succeed");
    let batches = plan_batches(
        scanned,
        &PlanOptions {
            batch_size_bytes: 25,
            max_files: Some(10),
        },
    )
    .expect("planning should succeed");

    assert_eq!(batches.len(), 2);
    let first: Vec<_> = batches[0]
        .files
        .iter()
        .map(|f| f.relative_path.to_string_lossy().to_string())
        .collect();
    let second: Vec<_> = batches[1]
        .files
        .iter()
        .map(|f| f.relative_path.to_string_lossy().to_string())
        .collect();

    assert_eq!(first, vec!["a/1.txt", "a/2.txt"]);
    assert_eq!(second, vec!["b/1.txt", "b/2.txt"]);
}

#[test]
fn planning_is_deterministic_across_runs() {
    let tmp = TempDir::new().expect("temp dir");
    create_file(tmp.path(), "x/c.txt", 7);
    create_file(tmp.path(), "x/a.txt", 5);
    create_file(tmp.path(), "y/b.txt", 6);

    let options = PlanOptions {
        batch_size_bytes: 10,
        max_files: Some(2),
    };
    let first = build_plan(tmp.path(), &options).expect("first plan should succeed");
    let second = build_plan(tmp.path(), &options).expect("second plan should succeed");

    let first_paths: Vec<Vec<String>> = first
        .batches
        .iter()
        .map(|b| {
            b.files
                .iter()
                .map(|f| f.relative_path.to_string_lossy().to_string())
                .collect()
        })
        .collect();
    let second_paths: Vec<Vec<String>> = second
        .batches
        .iter()
        .map(|b| {
            b.files
                .iter()
                .map(|f| f.relative_path.to_string_lossy().to_string())
                .collect()
        })
        .collect();

    assert_eq!(first_paths, second_paths);
    assert_eq!(first.batches, second.batches);
}

#[test]
#[should_panic(expected = "Could not locate batch batch-000002 in source directory")]
fn load_batch_definition_fails_for_second_batch_with_wrong_batch_size() {
    // THIS TEST DEMONSTRATES THE CURRENT BUG
    let tmp = TempDir::new().expect("temp dir");

    // Create 4 test files that will generate 4 batches with small size
    for i in 0..4 {
        create_file(tmp.path(), &format!("file{}.txt", i), 10);
    }

    // ✅ First we build with small batch size = 4 batches total
    let opts = PlanOptions {
        batch_size_bytes: 10,
        max_files: None,
    };

    let _plan = build_plan(tmp.path(), &opts).expect("plan built");

    // ❌ Current bug: using wrong batch size u64::MAX merges everything into 1 batch!
    // So it will only ever find batch-000001, not 000002
    let _batch = load_batch_definition(tmp.path(), "batch-000002", u64::MAX, None)
        .expect("this should fail");
}

#[test]
fn load_batch_definition_uses_original_max_files_for_deterministic_batches() {
    let tmp = TempDir::new().expect("temp dir");
    create_file(tmp.path(), "a.txt", 1);
    create_file(tmp.path(), "b.txt", 1);
    create_file(tmp.path(), "c.txt", 1);

    let batch = load_batch_definition(tmp.path(), "batch-000002", 10, Some(2))
        .expect("second batch should be reproducible with original max-files");

    assert_eq!(batch.id, "batch-000002");
    assert_eq!(batch.file_count, 1);
    assert_eq!(
        batch.files[0].relative_path,
        std::path::PathBuf::from("c.txt")
    );
}

#[test]
fn build_plan_handles_large_flat_tree() {
    let tmp = TempDir::new().expect("temp dir");
    for idx in 0..2000 {
        create_file(tmp.path(), &format!("flat/file-{idx:04}.txt"), 1);
    }

    let plan = build_plan(
        tmp.path(),
        &PlanOptions {
            batch_size_bytes: 500,
            max_files: Some(250),
        },
    )
    .expect("planning should succeed for large flat tree");

    assert_eq!(plan.source_file_count, 2000);
    assert_eq!(plan.source_total_bytes, 2000);
    assert_eq!(plan.batches.len(), 8);
}
