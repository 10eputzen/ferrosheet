//! Recently opened files, newest first, kept in a small text file in the user's
//! configuration folder (one path per line).

use std::path::{Path, PathBuf};

const MAX: usize = 10;

#[derive(Default)]
pub struct Recent {
    /// Where the list is stored; `None` keeps it in memory only (tests).
    file: Option<PathBuf>,
    pub files: Vec<PathBuf>,
}

impl Recent {
    pub fn load() -> Recent {
        let file = store();
        let files = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .map(|s| parse(&s))
            .unwrap_or_default();
        Recent { file, files }
    }

    /// Moves `path` to the top of the list and saves it.
    pub fn push(&mut self, path: &Path) {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        self.files.retain(|p| p != &path);
        self.files.insert(0, path);
        self.files.truncate(MAX);
        if let Some(f) = &self.file {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let body: Vec<_> = self
                .files
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            let _ = std::fs::write(f, body.join("\n"));
        }
    }

    /// The files that still exist.
    pub fn existing(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter().filter(|p| p.is_file())
    }
}

fn parse(s: &str) -> Vec<PathBuf> {
    s.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .take(MAX)
        .collect()
}

fn store() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?)
    } else if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
        PathBuf::from(x)
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config")
    };
    Some(dir.join("ferrosheet").join("recent.txt"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates_and_capped() {
        let mut r = Recent::default();
        for i in 0..12 {
            r.push(Path::new(&format!("/x/{i}.xlsx")));
        }
        r.push(Path::new("/x/5.xlsx"));
        assert_eq!(r.files.len(), MAX);
        assert!(r.files[0].ends_with("5.xlsx"));
        assert_eq!(r.files.iter().filter(|p| p.ends_with("5.xlsx")).count(), 1);
    }

    #[test]
    fn the_stored_list_skips_blank_lines() {
        assert_eq!(parse("a.xlsx\n\n  b.csv \n").len(), 2);
    }
}
