// Copyright 2022 Oxide Computer Company

use std::{
    fs::{File, OpenOptions},
    io::Read,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use clap::{Parser, ValueEnum};
use openapiv3::OpenAPI;
use progenitor::{GenerationSettings, Generator, InterfaceStyle, TagStyle};
use quote::quote;

pub mod built_info {
    // The file has been placed there by the build script.
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

/// Determine if current version is a pre-release or was built from a git-repo
fn release_is_unstable() -> bool {
    !built_info::PKG_VERSION_PRE.is_empty() || built_info::GIT_VERSION.is_some()
}

#[derive(Parser)]
struct Args {
    /// OpenAPI definition document (JSON)
    #[clap(short = 'i', long)]
    input: String,
    /// Output directory for Rust crate
    #[clap(short = 'o', long)]
    output: String,
    /// Emit Cargo.toml
    #[clap(long, default_value="false")]
    output_cargo_toml: bool,
    /// Target Rust crate name
    #[clap(short = 'n', long)]
    name: Option<String>,
    /// Target Rust crate version
    #[clap(short = 'v', long)]
    version: Option<String>,

    /// SDK interface style
    #[clap(value_enum, long, default_value_t = InterfaceArg::Positional)]
    interface: InterfaceArg,
    /// SDK tag style
    #[clap(value_enum, long, default_value_t = TagArg::Merged)]
    tags: TagArg,
    /// Include client
    #[clap(default_value = match release_is_unstable() { true => "true", false => "false" }, long, action = clap::ArgAction::Set)]
    include_client: Option<bool>,
}

#[derive(Copy, Clone, ValueEnum)]
enum InterfaceArg {
    Positional,
    Builder,
}

impl From<InterfaceArg> for InterfaceStyle {
    fn from(arg: InterfaceArg) -> Self {
        match arg {
            InterfaceArg::Positional => InterfaceStyle::Positional,
            InterfaceArg::Builder => InterfaceStyle::Builder,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum TagArg {
    Merged,
    Separate,
}

impl From<TagArg> for TagStyle {
    fn from(arg: TagArg) -> Self {
        match arg {
            TagArg::Merged => TagStyle::Merged,
            TagArg::Separate => TagStyle::Separate,
        }
    }
}

fn save<P>(p: P, data: &str) -> Result<()>
where
    P: AsRef<Path>,
{
    let p = p.as_ref();
    let mut f = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(p)?;
    f.write_all(data.as_bytes())?;
    f.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let args = Args::parse();
    let api = load_api(&args.input)?;
    let include_client = match args.include_client {
        Some(true) => true,
        Some(false) => false,
        None => true,
    };

    let mut settings = GenerationSettings::default();

    if !include_client {
        if release_is_unstable() {
            settings.use_client("*".to_string());
        } else {
            settings.use_client(built_info::PKG_VERSION.to_string());
        }
    }

    let mut builder = Generator::new(
        settings
            .with_interface(args.interface.into())
            .with_tag(args.tags.into()),
    );

    match builder.generate_text(&api) {
        Ok(api_code) => {
            let type_space = builder.get_type_space();

            println!("-----------------------------------------------------");
            println!(" TYPE SPACE");
            println!("-----------------------------------------------------");
            for (idx, type_entry) in type_space.iter_types().enumerate() {
                let n = type_entry.describe();
                println!("{:>4}  {}", idx, n);
            }
            println!("-----------------------------------------------------");
            println!();

            /*
             * Create the top-level crate directory:
             */
            let root = PathBuf::from(&args.output);
            std::fs::create_dir_all(&root)?;

            if args.output_cargo_toml {
                /*
                * Write the Cargo.toml file:
                */
                let name = &args.name.unwrap();
                let version = &args.version.unwrap();
                let mut toml = root.clone();
                toml.push("Cargo.toml");

                let tomlout = format!(
                    "[package]\n\
                    name = \"{}\"\n\
                    version = \"{}\"\n\
                    edition = \"2021\"\n\
                    \n\
                    [dependencies]\n\
                    {}\n\
                    \n",
                    name,
                    version,
                    builder.dependencies().join("\n"),
                );

                save(&toml, tomlout.as_str())?;
            }

            /*
             * Create the src/ directory:
             */
            let mut src = root;
            src.push("src");
            std::fs::create_dir_all(&src)?;

            /*
             * Create the Rust source file containing the generated client:
             */
            let lib_code = format!("mod progenitor_client;\n\n{}", api_code);
            let mut librs = src.clone();
            librs.push("lib.rs");
            save(librs, lib_code.as_str())?;

            /*
             * Create the Rust source file containing the support code:
             */
            let progenitor_client_code = match include_client {
                true => progenitor_client::code().to_string(),
                false => quote! {
                    pub use progenitor_client::{
                        ByteStream, ResponseValue, Error, RequestBuilderExt, encode_path
                    };
                }.to_string(),
            };
            let mut clientrs = src;
            clientrs.push("progenitor_client.rs");
            save(clientrs, &progenitor_client_code)?;
        }

        Err(e) => {
            println!("gen fail: {:?}", e);
            bail!("generation experienced errors");
        }
    }

    Ok(())
}

pub fn load_api<P>(p: P) -> Result<OpenAPI>
where
    P: AsRef<Path>,
{
    let mut f = File::open(p)?;

    let mut buf = [b' '];
    while buf[0].is_ascii_whitespace() {
        f.read_exact(&mut buf)?;
    }
    let reader = buf.as_ref().chain(f);

    let api = if buf[0] == b'{' {
        serde_json::from_reader(reader)?
    } else {
        serde_yaml::from_reader(reader)?
    };
    Ok(api)
}
