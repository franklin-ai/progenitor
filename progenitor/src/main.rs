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

    let mut builder = Generator::new(
        GenerationSettings::default()
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
            let progenitor_client_code = progenitor_client::code();
            let mut clientrs = src;
            clientrs.push("progenitor_client.rs");
            save(clientrs, progenitor_client_code)?;
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
