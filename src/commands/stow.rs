use std::env::current_dir;
use std::os::unix::fs::symlink;
use std::path::Path;

use crate::cli;
use crate::error::Error;
use crate::fs::{Base, Target, relative_path};

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
            println!("Stowing package: {}", pkg);
        }
        do_stow(&package_dir, &target_dir, &pkg, args.verbose, args.simulate)?;

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
    simulate: bool,
) -> Result<(), Error> {
    let _ = simulate;
    let _ = verbose;

    let package_path = package_dir.join(pkg);
    let link_target_base = relative_path(Target(&package_path), Base(&target_dir))?;
    if verbose {
        println!("target base: {:?}", link_target_base);
    }
    let mut iter = package_path.read_dir()?;
    while let Some(item) = iter.next() {
        let item = item?.file_name();
        if verbose {
            println!("stow::run: Stowing item: {}", item.to_string_lossy());
        }

        let link_path = target_dir.join(&item);
        // this is the base directory of the link targets
        let link_target = link_target_base.join(&item);
        if simulate {
            println!("symlink({:?}, {:?})", link_path, link_target);
        } else {
            symlink(&link_path, &link_target)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // use super::*;
}
