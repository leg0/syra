use std::env::current_dir;
use std::fs::read_link;
use std::path::Path;

use crate::cli;
use crate::error::Error;
use crate::fs::{Base, Package, Symlink, Target, relative_path, symlink};

pub fn run(args: cli::StowArgs) -> Result<(), Error> {
    if args.packages.is_empty() {
        eprintln!("error: At least one package is required");
        return Err(Error::MissingPackages);
    }
    if args.verbose {
        println!(
            "Stowing packages {:?}, src={:?}, dst={:?}",
            args.packages, args.package_dir, args.target_dir
        );
    }

    let cwd = current_dir()?;
    let package_dir = args
        .package_dir
        .unwrap_or_else(|| cwd.clone())
        .canonicalize()?;

    let target_dir = args
        .target_dir
        .unwrap_or(cwd)
        .parent()
        .map_or_else(|| Err(Error::DefaultTargetNotAvailable), Ok)?
        .canonicalize()?;

    for pkg in args.packages.iter() {
        if args.verbose {
            println!("Stowing package: {}", pkg);
        }

        let actions = do_stow(&package_dir, &target_dir, &pkg, args.verbose)?;

        for Symlink { path, target } in &actions {
            if args.verbose {
                println!("Creating symlink: {:?} -> {:?}", path, target);
            }
            if args.simulate {
                println!("symlink({:?}, {:?})", path, target);
            } else {
                symlink(&target, &path)?;
            }
        }

        if args.verbose {
            println!("Stowed package: {}", pkg);
        }
    }

    Ok(())
}

fn do_stow(
    package_dir: &Path,
    target_dir: &Path,
    pkg: &str,
    verbose: bool,
) -> Result<Vec<Symlink>, Error> {
    let package_path = package_dir.join(pkg);
    let link_target_base = relative_path(Target(&package_path), Base(&target_dir))?;
    if verbose {
        println!("target base: {:?}", link_target_base);
    }

    let mut actions = Vec::new();

    let package = Package::new(package_dir, pkg)?;
    for item in package.get_package_contents()? {
        if verbose {
            println!("stow::run: Stowing item: {}", item.to_string_lossy());
        }

        let link_path = target_dir.join(&item);
        // this is the base directory of the link targets
        let link_target = link_target_base.join(&item);

        // It is ok if the link path already exists, if either:
        // - it is a directory
        // - it is a symlink that points to the same target

        if link_path.is_dir() {
            todo!("Handle existing directory at link path: {:?}", link_path);
            // Try to create symlinks into that directory such that
            // target/dir/item -> package/dir/item

            // Do this until all links have been created, or an error occurs.
        } else if link_path.is_symlink() {
            let existing_target = read_link(&link_path)?;
            if existing_target == link_target {
                if verbose {
                    println!(
                        "symlink({:?}, {:?}) already exists and points to the same target",
                        link_path, link_target
                    );
                }
            } else {
                return Err(Error::LinkNotOwnedByPackage(link_path, pkg.to_string()));
            }
        } else if link_path.is_file() {
            eprintln!(
                "error: Link path {:?} already exists and is not a directory or symlink",
                link_path
            );
            return Err(Error::LinkPathExists(link_path));
        }

        actions.push(Symlink {
            path: link_path,
            target: link_target,
        });
    }

    Ok(actions)
}

#[cfg(test)]
mod tests {
    // use super::*;
}
