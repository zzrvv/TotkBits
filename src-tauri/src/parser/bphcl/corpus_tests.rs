use super::BphclDocument;
use roead::{
    sarc::{Sarc, SarcWriter},
    Endian,
};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn corpus() -> Vec<(String, BphclDocument)> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tmp/_bphcl");
    let mut paths: Vec<_> = fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
        .map(|entry| entry.expect("failed to read corpus entry").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "bphcl")
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "BPHCL corpus is empty");
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .expect("corpus file has no name")
                .to_string_lossy()
                .into_owned();
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            let document = BphclDocument::parse(&bytes)
                .unwrap_or_else(|error| panic!("failed to parse {name}: {error}"));
            document
                .validate_item_graph()
                .unwrap_or_else(|error| panic!("invalid ITEM/PTCH graph in {name}: {error}"));
            (name, document)
        })
        .collect()
}

fn validate_merged(label: &str, bytes: &[u8]) -> BphclDocument {
    let document = BphclDocument::parse(bytes)
        .unwrap_or_else(|error| panic!("failed to reparse {label}: {error}"));
    document
        .validate_item_graph()
        .unwrap_or_else(|error| panic!("invalid merged ITEM/PTCH graph in {label}: {error}"));
    document
}

#[test]
fn corpus_projects_to_format_neutral_graphs() {
    for (name, document) in corpus() {
        let graph = document.neutral_physics_graph();
        assert_eq!(graph.cloths.len(), document.cloth.len(), "{name}");
        assert_eq!(graph.skeletons.len(), document.skeletons.len(), "{name}");
        assert_eq!(
            graph.collidables.len(),
            document.collidables.len(),
            "{name}"
        );
        assert!(graph
            .cloths
            .iter()
            .all(|cloth| cloth.simulations.iter().all(|simulation| simulation
                .particles
                .iter()
                .all(|particle| particle.position.is_some()))));
    }
}

#[test]
fn cloth_and_collidable_removals_reparse_across_corpus_samples() {
    let documents = corpus();
    let (cloth_name, cloth_document) = documents
        .iter()
        .find(|(_, document)| document.cloth.len() > 1 && !document.collidables.is_empty())
        .expect("corpus has no BPHCL with removable cloth");
    let cloth_bytes = cloth_document
        .remove_cloth(1)
        .unwrap_or_else(|error| panic!("failed to remove cloth from {cloth_name}: {error}"));
    let cloth_result = validate_merged("cloth removal", &cloth_bytes);
    assert_eq!(cloth_result.cloth.len(), cloth_document.cloth.len() - 1);

    let (collidable_name, collidable_document) = documents
        .iter()
        .find(|(_, document)| document.collidables.len() > 1 && !document.cloth.is_empty())
        .expect("corpus has no BPHCL with removable collidable");
    let collidable_bytes = collidable_document
        .remove_collidable(1)
        .unwrap_or_else(|error| {
            panic!("failed to remove collidable from {collidable_name}: {error}")
        });
    let collidable_result = validate_merged("collidable removal", &collidable_bytes);
    assert_eq!(
        collidable_result.collidables.len(),
        collidable_document.collidables.len() - 1
    );
}

#[test]
#[ignore = "full tmp/_bphcl corpus matrix takes about 90 seconds"]
fn corpus_merges_reparse_with_valid_pointers_and_survive_sarc_roundtrips() {
    let documents = corpus();
    let mut cloth_successes = 0;
    let mut collidable_successes = 0;
    let mut rejections = BTreeMap::<String, usize>::new();
    let mut archive_sample = None;

    for source_index in 0..documents.len() {
        let target_index = (source_index + 1) % documents.len();
        let (source_name, source) = &documents[source_index];
        let (target_name, target) = &documents[target_index];
        if !source.cloth.is_empty() {
            let label = format!("cloth {source_name} -> {target_name}");
            match target.merge_complete_cloth(source, 0) {
                Ok(bytes) => {
                    validate_merged(&label, &bytes);
                    cloth_successes += 1;
                    archive_sample.get_or_insert(bytes);
                }
                Err(error) => {
                    *rejections
                        .entry(rejection_category(&error.to_string()))
                        .or_default() += 1
                }
            }
        }
        if !source.collidables.is_empty() {
            let label = format!("collidable {source_name} -> {target_name}");
            match target.merge_collidable(source, 0) {
                Ok(bytes) => {
                    validate_merged(&label, &bytes);
                    collidable_successes += 1;
                    archive_sample.get_or_insert(bytes);
                }
                Err(error) => {
                    *rejections
                        .entry(rejection_category(&error.to_string()))
                        .or_default() += 1
                }
            }
        }
    }

    assert!(
        cloth_successes > 0,
        "no complete-cloth corpus merge succeeded"
    );
    assert!(
        collidable_successes > 0,
        "no standalone-collidable corpus merge succeeded"
    );
    let merged = archive_sample.expect("no merged archive sample was produced");

    let mut root = SarcWriter::new(Endian::Little);
    root.add_file("Physics/merged.bphcl", merged.clone());
    let reopened_root = Sarc::new(root.to_binary()).expect("failed to reopen root SARC");
    validate_merged(
        "root SARC roundtrip",
        reopened_root
            .get_data("Physics/merged.bphcl")
            .expect("merged BPHCL missing after root SARC reopen"),
    );

    let mut inner = SarcWriter::new(Endian::Little);
    inner.add_file("Physics/merged.bphcl", merged);
    let mut outer = SarcWriter::new(Endian::Little);
    outer.add_file("Nested/physics.pack", inner.to_binary());
    let reopened_outer = Sarc::new(outer.to_binary()).expect("failed to reopen outer SARC");
    let inner_bytes = reopened_outer
        .get_data("Nested/physics.pack")
        .expect("nested SARC missing after outer reopen");
    let reopened_inner = Sarc::new(inner_bytes.to_vec()).expect("failed to reopen nested SARC");
    validate_merged(
        "nested SARC roundtrip",
        reopened_inner
            .get_data("Physics/merged.bphcl")
            .expect("merged BPHCL missing after nested SARC reopen"),
    );

    let rejected: usize = rejections.values().sum();
    eprintln!("validated {} corpus files; {cloth_successes} cloth merges and {collidable_successes} collidable merges succeeded; {rejected} incompatible pairs were rejected: {rejections:?}", documents.len());
}

fn rejection_category(message: &str) -> String {
    if message.contains("already has a different collidable named") {
        "different collidable with the same name".into()
    } else if message.contains("missing TYPE dependency") {
        "missing TYPE dependency".into()
    } else if message.contains("conflicting TYPE definition") {
        "conflicting TYPE definition".into()
    } else if message.contains("no paired skeleton") {
        "cloth has no paired skeleton".into()
    } else {
        message.into()
    }
}
