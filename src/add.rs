use std::fs;
use std::path::Path;
use std::path::PathBuf;

use regex::Regex;
use toml_edit::{Array, DocumentMut, Formatted, Value};
use walkdir::{DirEntry, WalkDir};

use crate::{Config, config::ConfigLoadError};

pub fn read_and_verify_config(config_file: impl AsRef<Path>) -> Result<DocumentMut, AddError> {
    let config_file = config_file.as_ref();
    let _ = Config::from_file(config_file).map_err(|e| AddError {
        path: config_file.into(),
        source: Box::new(AddErrorKind::ConfigLoad(e)),
    })?;
    let config_content = fs::read_to_string(config_file).unwrap(); // Verified config could be loaded above

    Ok(config_content.parse::<DocumentMut>().unwrap()) // Verify config was valid toml above
}

pub fn add_packages(config_doc: &mut DocumentMut, packages: Vec<String>) -> Result<(), AddError> {
    // get the dependencies array
    let config_deps = get_mut_array(config_doc);

    // collect the names of all of the dependencies
    let config_dep_names = config_deps
        .iter()
        .filter_map(|v| match v {
            Value::String(s) => Some(s.value().as_str()),
            Value::InlineTable(t) => t.get("name").and_then(|v| v.as_str()),
            _ => None,
        })
        .map(|s| s.to_string()) // Need to allocate so values are not a reference to a mut
        .collect::<Vec<_>>();

    // Determine if the dep to add is in the config, if not add it
    for d in packages {
        if !config_dep_names.contains(&d) {
            config_deps.push(Value::String(Formatted::new(d)));
            // Couldn't format value before pushing, so adding formatting after its added
            if let Some(last) = config_deps.iter_mut().last() {
                last.decor_mut().set_prefix("\n    ");
            }
        }
    }

    // Set a trailing new line and comma for the last element for proper formatting
    config_deps.set_trailing("\n");
    config_deps.set_trailing_comma(true);

    Ok(())
}

fn get_mut_array(doc: &mut DocumentMut) -> &mut Array {
    // the dependnecies array is behind the project table
    let deps = doc
        .get_mut("project")
        .and_then(|item| item.as_table_mut())
        .unwrap()
        .entry("dependencies")
        .or_insert_with(|| Array::new().into())
        .as_array_mut()
        .unwrap();

    // remove formatting on the last element as we will re-add
    if let Some(last) = deps.iter_mut().last() {
        last.decor_mut().set_suffix("");
    }
    deps
}

fn is_hidden(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| s.starts_with(".") && s.len() > 1)
        .unwrap_or(false)
}

pub fn scan_r_files_for_packages(dir: &PathBuf) -> Result<Vec<String>, std::io::Error> {
    let mut packages = Vec::new();
    let re = Regex::new(r#"(?:library|require)\(\s*["']?([A-Za-z0-9_.]+)["']?\s*\)"#).unwrap();

    const EXCLUDED: [&str; 1] = ["rv"];

    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| !is_hidden(e))
        .filter_map(|e| e.ok())
        .filter(|e| {
            let path = e.path();
            // Exclude if any ancestor folder or the file itself matches an excluded name
            !path.ancestors().any(|ancestor| {
                ancestor
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| EXCLUDED.contains(&n))
            })
        })
    {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("R"))
        {
            let content = std::fs::read_to_string(entry.path())?;
            for line in content.lines() {
                let trimmed = line.trim_start();
                if trimmed.starts_with('#') {
                    continue;
                }
                for cap in re.captures_iter(line) {
                    if let Some(pkg) = cap.get(1) {
                        packages.push(pkg.as_str().to_string());
                    }
                }
            }
        }
    }
    packages.sort();
    packages.dedup();
    Ok(packages)
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to edit config at `{path}`")]
#[non_exhaustive]
pub struct AddError {
    path: Box<Path>,
    source: Box<AddErrorKind>,
}

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub enum AddErrorKind {
    Io(#[from] std::io::Error),
    Parse(#[from] toml_edit::TomlError),
    ConfigLoad(#[from] ConfigLoadError),
}

#[cfg(test)]
mod tests {
    use crate::{add_packages, read_and_verify_config};

    #[test]
    fn add_remove() {
        let config_file = "src/tests/valid_config/all_fields.toml";
        let mut doc = read_and_verify_config(&config_file).unwrap();
        add_packages(&mut doc, vec!["pkg1".to_string(), "pkg2".to_string()]).unwrap();
        insta::assert_snapshot!("add_remove", doc.to_string());
    }
}
