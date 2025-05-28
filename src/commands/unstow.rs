use std::env::current_dir;
use std::path::Path;

use crate::error::Error;
use crate::fs::{relative_path, Base, Target};
use crate::{cli::UnstowArgs};

pub fn run(args: UnstowArgs) -> Result<(), Error> {
    if args.packages.is_empty() {
        eprintln!("error: At least one package is required");
        return Err(Error::MissingPackages);
    }

    let package_dir = args
        .package_dir
        .unwrap_or_else(|| current_dir().unwrap())
        .canonicalize()?;

    let target_dir = args
        .target_dir
        .unwrap_or_else(|| current_dir().unwrap().parent().unwrap().into())
        .canonicalize()?;

    for pkg in args.packages.iter() {
        if args.verbose {
            println!("Unstowing package: {}", pkg);
        }

        do_unstow(&package_dir, &target_dir, &pkg, args.verbose, args.simulate)?;

        if args.verbose {
            println!("Unstowed package: {}", pkg);
        }
    }


    Ok(())
}

fn do_unstow(
    package_dir: &Path,
    target_dir: &Path,
    pkg: &str,
    verbose: bool,
    simulate: bool,
) -> Result<(), Error> {
    let _ = (package_dir, target_dir, pkg, verbose, simulate);

    // Like stow, but instead of creating symlinks, we remove them
    // Before removing, make sure that the symlink points to the file/directory in the package
    // directory.

    
    Ok(())
}

fn is_owned_by_package(
    package_dir: &Path,
    target_dir: &Path,
    pkg: &str,
) -> Result<bool, Error> {
    let package_path = package_dir.join(pkg);
    let link_target_base = relative_path(Target(&package_path), Base(&target_dir))?;
    let _ = link_target_base;
    
    // Check if the symlink points to the package directory
    // This is a placeholder for the actual implementation
    Ok(true)
}
