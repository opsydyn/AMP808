//! Searchable local-first MilkDrop preset catalogue.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::milkdrop::{MilkdropPreset, MilkdropPresetError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PresetOrigin {
    Amp808,
    Local,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PresetSource {
    FirstLight,
    Local(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresetEntry {
    id: String,
    title: String,
    category: String,
    origin: PresetOrigin,
    source: PresetSource,
}

impl PresetEntry {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn category(&self) -> &str {
        &self.category
    }

    #[cfg(test)]
    pub(crate) const fn origin(&self) -> PresetOrigin {
        self.origin
    }

    pub(crate) fn load(&self) -> Result<MilkdropPreset, MilkdropPresetError> {
        match &self.source {
            PresetSource::FirstLight => Ok(MilkdropPreset::first_light()),
            PresetSource::Local(path) => MilkdropPreset::read(path),
        }
    }
}

#[derive(Debug)]
pub(crate) struct PresetCatalogError {
    path: PathBuf,
    source: io::Error,
}

impl PresetCatalogError {
    fn new(path: PathBuf, source: io::Error) -> Self {
        Self { path, source }
    }
}

impl fmt::Display for PresetCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "could not scan preset folder {}: {}",
            self.path.display(),
            self.source
        )
    }
}

impl std::error::Error for PresetCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PresetCatalog {
    entries: Vec<PresetEntry>,
}

impl PresetCatalog {
    pub(crate) fn built_in() -> Self {
        Self {
            entries: vec![PresetEntry {
                id: "amp808:first-light".to_owned(),
                title: "First Light".to_owned(),
                category: "AMP808".to_owned(),
                origin: PresetOrigin::Amp808,
                source: PresetSource::FirstLight,
            }],
        }
    }

    pub(crate) fn entries(&self) -> &[PresetEntry] {
        &self.entries
    }

    pub(crate) fn replace_local(&mut self, entries: Vec<PresetEntry>) {
        self.entries
            .retain(|entry| entry.origin == PresetOrigin::Amp808);
        self.entries.extend(entries);
    }

    pub(crate) fn categories(&self) -> Vec<String> {
        let mut categories = self
            .entries
            .iter()
            .map(|entry| entry.category.clone())
            .collect::<Vec<_>>();
        categories.sort_by_key(|category| category.to_lowercase());
        categories.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        categories
    }

    pub(crate) fn search(&self, query: &str, category: Option<&str>) -> Vec<usize> {
        let mut matches = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                category.is_none_or(|category| entry.category.eq_ignore_ascii_case(category))
            })
            .filter_map(|(index, entry)| {
                query
                    .split_whitespace()
                    .try_fold(0_i64, |total, term| {
                        [entry.title.as_str(), entry.category.as_str()]
                            .into_iter()
                            .filter_map(|field| fuzzy_score(term, field))
                            .max()
                            .map(|score| total.saturating_add(score))
                    })
                    .map(|score| (index, score))
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_index, left_score), (right_index, right_score)| {
            right_score.cmp(left_score).then_with(|| {
                let left = &self.entries[*left_index];
                let right = &self.entries[*right_index];
                compare_text(&left.category, &right.category)
                    .then_with(|| compare_text(&left.title, &right.title))
                    .then_with(|| left.id.cmp(&right.id))
            })
        });
        matches.into_iter().map(|(index, _)| index).collect()
    }

    pub(crate) fn discover_local(root: &Path) -> Result<Vec<PresetEntry>, PresetCatalogError> {
        let metadata = fs::metadata(root)
            .map_err(|error| PresetCatalogError::new(root.to_path_buf(), error))?;
        if !metadata.is_dir() {
            return Err(PresetCatalogError::new(
                root.to_path_buf(),
                io::Error::new(io::ErrorKind::InvalidInput, "path is not a directory"),
            ));
        }

        let mut directories = vec![root.to_path_buf()];
        let mut paths = Vec::new();
        while let Some(directory) = directories.pop() {
            let entries = fs::read_dir(&directory)
                .map_err(|error| PresetCatalogError::new(directory.clone(), error))?;
            for entry in entries {
                let entry =
                    entry.map_err(|error| PresetCatalogError::new(directory.clone(), error))?;
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .map_err(|error| PresetCatalogError::new(path.clone(), error))?;
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    directories.push(path);
                } else if file_type.is_file() && has_milk_extension(&path) {
                    paths.push(path);
                }
            }
        }
        paths.sort_by(|left, right| {
            left.strip_prefix(root)
                .unwrap_or(left)
                .cmp(right.strip_prefix(root).unwrap_or(right))
        });

        Ok(paths
            .into_iter()
            .map(|path| local_entry(root, path))
            .collect())
    }
}

fn compare_text(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_lowercase()
        .cmp(&right.to_lowercase())
        .then_with(|| left.cmp(right))
}

fn fuzzy_score(query: &str, candidate: &str) -> Option<i64> {
    let query = query.to_lowercase();
    if query.is_empty() {
        return Some(0);
    }

    let candidate = candidate.to_lowercase();
    let candidate_chars = candidate.chars().collect::<Vec<_>>();
    let mut cursor = 0;
    let mut previous_match = None;
    let mut score = 0_i64;

    for needle in query.chars() {
        let offset = candidate_chars[cursor..]
            .iter()
            .position(|character| *character == needle)?;
        let matched = cursor + offset;
        score += 10;
        if previous_match.is_some_and(|previous| matched == previous + 1) {
            score += 12;
        }
        if matched == 0
            || candidate_chars
                .get(matched.wrapping_sub(1))
                .is_some_and(|character| !character.is_alphanumeric())
        {
            score += 8;
        }
        score -= i64::try_from(offset).unwrap_or(i64::MAX);
        previous_match = Some(matched);
        cursor = matched + 1;
    }

    if candidate.contains(&query) {
        score += 40;
    }
    Some(score)
}

fn has_milk_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("milk"))
}

fn local_entry(root: &Path, path: PathBuf) -> PresetEntry {
    let relative = path.strip_prefix(root).unwrap_or(&path);
    let title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Local preset")
        .to_owned();
    let category = relative
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .and_then(|parent| parent.components().next())
        .and_then(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .unwrap_or("Uncategorised")
        .to_owned();
    let id = format!("local:{}", relative.to_string_lossy());
    PresetEntry {
        id,
        title,
        category,
        origin: PresetOrigin::Local,
        source: PresetSource::Local(path),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    use tempfile::tempdir;

    use super::{PresetCatalog, PresetOrigin};

    #[test]
    fn built_in_catalog_exposes_the_repository_owned_first_light_preset() {
        let catalog = PresetCatalog::built_in();
        let entry = catalog
            .entries()
            .first()
            .expect("the built-in catalogue contains First Light");

        assert_eq!(entry.id(), "amp808:first-light");
        assert_eq!(entry.title(), "First Light");
        assert_eq!(entry.category(), "AMP808");
        assert_eq!(entry.origin(), PresetOrigin::Amp808);
        assert_eq!(
            entry.load().expect("First Light loads").name(),
            "First Light"
        );
    }

    #[test]
    fn local_discovery_is_recursive_categorised_and_deterministic() {
        let directory = tempdir().expect("temporary preset directory is created");
        let root = directory.path().join("presets");
        fs::create_dir_all(root.join("Fractal/Deep")).expect("nested preset category is created");
        fs::create_dir_all(root.join("Particles")).expect("preset category is created");
        fs::write(root.join("Fractal/Deep/Spiral.MILK"), "[preset00]\n")
            .expect("nested preset is written");
        fs::write(root.join("Particles/Bloom.milk"), "[preset00]\n")
            .expect("particle preset is written");
        fs::write(root.join("Loose.milk"), "[preset00]\n").expect("root preset is written");
        fs::write(root.join("notes.txt"), "not a preset").expect("non-preset is written");

        #[cfg(unix)]
        {
            let outside = directory.path().join("outside");
            fs::create_dir(&outside).expect("outside directory is created");
            fs::write(outside.join("Hidden.milk"), "[preset00]\n")
                .expect("outside preset is written");
            symlink(&outside, root.join("Linked")).expect("symlinked preset directory is created");
        }

        let entries = PresetCatalog::discover_local(&root).expect("local presets are discovered");
        let summary = entries
            .iter()
            .map(|entry| {
                (
                    entry.title().to_owned(),
                    entry.category().to_owned(),
                    entry.origin(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            summary,
            [
                (
                    "Spiral".to_owned(),
                    "Fractal".to_owned(),
                    PresetOrigin::Local
                ),
                (
                    "Loose".to_owned(),
                    "Uncategorised".to_owned(),
                    PresetOrigin::Local
                ),
                (
                    "Bloom".to_owned(),
                    "Particles".to_owned(),
                    PresetOrigin::Local
                ),
            ]
        );
    }

    #[test]
    fn catalogue_search_combines_fuzzy_terms_with_category_filters() {
        let directory = tempdir().expect("temporary preset directory is created");
        let root = directory.path();
        fs::create_dir(root.join("Fractal")).expect("fractal category is created");
        fs::create_dir(root.join("Particles")).expect("particle category is created");
        fs::write(root.join("Fractal/Cosmic Spiral.milk"), "[preset00]\n")
            .expect("fractal preset is written");
        fs::write(root.join("Particles/Cosmic Bloom.milk"), "[preset00]\n")
            .expect("particle preset is written");

        let local = PresetCatalog::discover_local(root).expect("local presets are discovered");
        let mut catalog = PresetCatalog::built_in();
        catalog.replace_local(local);

        let fuzzy_titles = catalog
            .search("csmc sprl frctl", None)
            .into_iter()
            .map(|index| catalog.entries()[index].title())
            .collect::<Vec<_>>();
        assert_eq!(fuzzy_titles, ["Cosmic Spiral"]);

        let particle_titles = catalog
            .search("cosmic", Some("Particles"))
            .into_iter()
            .map(|index| catalog.entries()[index].title())
            .collect::<Vec<_>>();
        assert_eq!(particle_titles, ["Cosmic Bloom"]);
        assert_eq!(catalog.categories(), ["AMP808", "Fractal", "Particles"]);
    }
}
