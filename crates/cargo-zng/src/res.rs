use std::{
    fs, io,
    ops::ControlFlow,
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{bail, Context as _};

use built_in::{display_path, ZR_WORKSPACE_DIR};
use clap::*;
use color_print::cstr;

use crate::util;

use self::tool::Tools;

pub mod built_in;
mod tool;

#[derive(Args, Debug)]
pub struct ResArgs {
    /// Resources source dir
    #[arg(default_value = "assets")]
    source: PathBuf,
    /// Resources target dir
    ///
    /// This directory is wiped before each build.
    #[arg(default_value = "target/assets")]
    target: PathBuf,

    /// Copy all static files to the target dir
    #[arg(long, action)]
    pack: bool,

    /// Search for `zng-res-{tool}` in this directory first
    #[arg(long, default_value = "tools")]
    tools: PathBuf,
    /// Prints help for all tools available
    #[arg(long, action)]
    list: bool,

    /// Tool cache dir
    #[arg(long, default_value = "target/assets.cache")]
    tool_cache: PathBuf,

    /// Number of build passes allowed before final
    #[arg(long, default_value = "32")]
    recursion_limit: u32,
}

fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|e| fatal!("cannot resolve path, {e}"))
}

pub(crate) fn run(mut args: ResArgs) {
    if args.tools.exists() {
        args.tools = canonicalize(&args.tools);
    }
    if args.list {
        return list(&args.tools);
    }

    if !args.source.exists() {
        fatal!("source dir does not exist");
    }
    if let Err(e) = fs::create_dir_all(&args.tool_cache) {
        fatal!("cannot create cache dir, {e}");
    }
    if let Err(e) = fs::remove_dir_all(&args.target) {
        if e.kind() != io::ErrorKind::NotFound {
            fatal!("cannot remove target dir, {e}");
        }
    }
    if let Err(e) = fs::create_dir_all(&args.target) {
        fatal!("cannot create target dir, {e}");
    }

    args.source = canonicalize(&args.source);
    args.target = canonicalize(&args.target);
    args.tool_cache = canonicalize(&args.tool_cache);

    // tool request paths are relative to the workspace root
    if let Some(p) = util::workspace_dir() {
        if let Err(e) = std::env::set_current_dir(p) {
            fatal!("cannot change dir, {e}");
        }
    } else {
        warn!("source is not in a Cargo workspace, tools will run using source as root");
        if let Err(e) = std::env::set_current_dir(&args.source) {
            fatal!("cannot change dir, {e}");
        }
    }
    // to use `display_path` in the tool runner (current process)
    std::env::set_var(ZR_WORKSPACE_DIR, std::env::current_dir().unwrap());

    let start = Instant::now();
    if let Err(e) = build(&args) {
        let e = e.to_string();
        for line in e.lines() {
            eprintln!("   {line}");
        }
        fatal!("res build failed");
    }

    println!(cstr!("<bold><green>Finished</green></bold> res build in {:?}"), start.elapsed());
    println!("         {}", args.target.display());
}

fn build(args: &ResArgs) -> anyhow::Result<()> {
    let tools = Tools::capture(&args.tools, args.tool_cache.clone())?;
    source_to_target_pass(args, &tools, &args.source, &args.target)?;

    let mut passes = 0;
    while target_to_target_pass(args, &tools, &args.target)? {
        passes += 1;
        if passes >= args.recursion_limit {
            bail!("reached --recursion-limit of {}", args.recursion_limit)
        }
    }

    tools.run_final(&args.source, &args.target)
}

fn source_to_target_pass(args: &ResArgs, tools: &Tools, source: &Path, target: &Path) -> anyhow::Result<()> {
    for entry in walkdir::WalkDir::new(source).min_depth(1).max_depth(1).sort_by_file_name() {
        let entry = entry.with_context(|| format!("cannot read dir entry {}", source.display()))?;
        if entry.file_type().is_dir() {
            let source = entry.path();
            // mirror dir in target
            println!("{}", display_path(source));
            let target = target.join(source.file_name().unwrap());
            fs::create_dir(&target).with_context(|| format!("cannot create_dir {}", target.display()))?;
            println!("  {}", display_path(&target));

            source_to_target_pass(args, tools, source, &target)?;
        } else if entry.file_type().is_file() {
            let source = entry.path();

            // run tool
            if let Some(ext) = source.extension() {
                let ext = ext.to_string_lossy();
                if let Some(tool) = ext.strip_prefix("zr-") {
                    println!("{}", display_path(source));
                    let output = tools.run(tool, &args.source, &args.target, source)?;
                    for line in output.lines() {
                        println!("  {line}");
                    }
                    continue;
                }
            }

            // or pack
            if args.pack {
                println!("{}", display_path(source));
                let target = target.join(source.file_name().unwrap());
                fs::copy(source, &target).with_context(|| format!("cannot copy {} to {}", source.display(), target.display()))?;
                println!("  {}", display_path(&target));
            }
        } else if entry.file_type().is_symlink() {
            warn!("symlink ignored in `{}`, use zr-tools to 'link'", entry.path().display());
        }
    }
    Ok(())
}

fn target_to_target_pass(args: &ResArgs, tools: &Tools, dir: &Path) -> anyhow::Result<bool> {
    let mut any = false;
    for entry in walkdir::WalkDir::new(dir).min_depth(1).sort_by_file_name() {
        let entry = entry.with_context(|| format!("cannot read dir entry {}", dir.display()))?;
        if entry.file_type().is_file() {
            let path = entry.path();
            // run tool
            if let Some(ext) = path.extension() {
                let ext = ext.to_string_lossy();
                if let Some(tool) = ext.strip_prefix("zr-") {
                    any = true;
                    println!("{}", display_path(path));
                    let tool_r = tools.run(tool, &args.source, &args.target, path);
                    fs::remove_file(path)?;
                    let output = tool_r?;
                    for line in output.lines() {
                        println!("  {line}");
                    }
                }
            }
        }
    }
    Ok(any)
}

fn list(tools: &Path) {
    let r = tool::visit_tools(tools, |tool| {
        println!(cstr!("<bold>.zr-{}</bold> @ {}"), tool.name, tool.path.display());
        match tool.help() {
            Ok(h) => {
                for line in h.trim().lines() {
                    println!("  {line}");
                }
                if !h.is_empty() {
                    println!();
                }
            }
            Err(e) => error!("{e}"),
        }

        Ok(ControlFlow::Continue(()))
    });
    if let Err(e) = r {
        fatal!("{e}")
    }
}