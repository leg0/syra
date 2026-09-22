use crate::cli::StowArgs;
use crate::error::Error;
use crate::plan::Plan;

use super::{stow, unstow};

pub fn run(args: &StowArgs) -> Result<(), Error> {
    let plan = Plan::from_args(args)?;
    let plan = unstow::plan(plan, args)?;
    let plan = stow::plan(plan, args)?;
    plan.execute(args.simulate, args.verbose)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{File, create_dir_all, remove_dir_all};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestTree {
        root: PathBuf,
    }

    impl TestTree {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("syra-restow-{unique}"));
            create_dir_all(root.join("stow").join("package").join("bin")).unwrap();
            File::create(
                root.join("stow")
                    .join("package")
                    .join("bin")
                    .join("tool.exe"),
            )
            .unwrap();
            Self { root }
        }

        fn args(&self) -> StowArgs {
            StowArgs {
                package_dir: Some(self.root.join("stow")),
                target_dir: Some(self.root.clone()),
                packages: vec!["package".to_string()],
                verbose: false,
                simulate: false,
            }
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn failed_restow_does_not_remove_existing_installation() {
        let tree = TestTree::new();
        let args = tree.args();
        stow::run(&args).unwrap();
        File::create(tree.root.join("stow").join("package").join("conflict.txt")).unwrap();
        File::create(tree.root.join("conflict.txt")).unwrap();

        assert!(run(&args).is_err());
        assert!(tree.root.join("bin").is_symlink());
        assert!(tree.root.join("bin").join("tool.exe").exists());
    }

    #[test]
    fn restow_preserves_ignore_filtering() {
        let tree = TestTree::new();
        let package = tree.root.join("stow").join("package");
        File::create(package.join("bin").join("ignored.tmp")).unwrap();
        std::fs::write(package.join(".stow-local-ignore"), ".*\\.tmp\n").unwrap();
        let args = tree.args();

        stow::run(&args).unwrap();
        run(&args).unwrap();

        assert!(tree.root.join("bin").is_dir());
        assert!(tree.root.join("bin").join("tool.exe").is_symlink());
        assert!(!tree.root.join("bin").join("ignored.tmp").exists());
    }
}
