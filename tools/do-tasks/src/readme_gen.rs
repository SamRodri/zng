use crate::{println, util};
use std::{borrow::Cow, collections::HashSet, path::PathBuf};

pub fn generate(args: Vec<&str>) {
    for member in &util::publish_members() {
        if !args.is_empty() && !args.contains(&member.name.as_str()) {
            continue;
        }

        let readme = if member.name == "zng" {
            PathBuf::from("zng/../README.md")
        } else {
            PathBuf::from(format!("{}/README.md", member.name))
        };

        println(&format!("{}/Cargo.toml", member.name));

        let previous = if readme.exists() {
            Cow::from(std::fs::read_to_string(&readme).unwrap())
        } else {
            Cow::from(README_TEMPLATE.to_owned())
        };

        let mut s = String::new();
        let mut lines = previous.lines().peekable();
        while let Some(line) = lines.next() {
            use std::fmt::*;

            writeln!(&mut s, "{line}").unwrap();
            match line {
                "<!--do doc --readme header-->" => {
                    writeln!(&mut s, "{HEADER}").unwrap();
                    while let Some(l) = lines.next() {
                        if l.trim().is_empty() {
                            break;
                        }
                    }
                }
                "<!--do doc --readme features-->" => {
                    if let Some(l) = lines.peek() {
                        if l.trim() == FEATURES_HEADER {
                            while let Some(l) = lines.next() {
                                if l == SECTION_END {
                                    break;
                                }
                            }
                        }
                    }

                    let (features, defaults) = read_features(&format!("{}/Cargo.toml", member.name));
                    if !features.is_empty() {
                        writeln!(&mut s, "{FEATURES_HEADER}").unwrap();

                        if features.len() == 1 {
                            if defaults.contains(&features[0].name) {
                                writeln!(&mut s, "\n This crate provides 1 feature flag, enabled by default.",).unwrap();
                            } else {
                                writeln!(&mut s, "\n This crate provides 1 feature flag, not enabled by default.",).unwrap();
                            }
                        } else {
                            writeln!(
                                &mut s,
                                "\nThis crate provides {} feature flags, {} enabled by default.\n",
                                features.len(),
                                defaults.len(),
                            )
                            .unwrap();
                        }

                        for f in features {
                            if f.docs.is_empty() {
                                crate::error(format_args!("missing docs for `{}` feature", f.name));
                            }
                            writeln!(&mut s, "#### `\"{}\"`\n{}", f.name, f.docs).unwrap();
                            if defaults.contains(&f.name) {
                                writeln!(&mut s, "*Enabled by default.*\n").unwrap();
                            }
                        }

                        writeln!(&mut s, "{SECTION_END}").unwrap();
                    }
                }
                _ => {}
            }
        }

        if s != previous {
            std::fs::write(&readme, s.as_bytes()).unwrap();

            if previous == README_TEMPLATE {
                println("    generated");
            } else {
                println("    updated");
            }
        }
    }
}

struct Feature {
    name: String,
    docs: String,
}

fn read_features(cargo: &str) -> (Vec<Feature>, HashSet<String>) {
    let cargo = std::fs::read_to_string(cargo).unwrap();
    let mut r = vec![];
    let mut rd = HashSet::new();
    let mut in_features = false;

    let mut next_docs = String::new();

    let rgx = regex::Regex::new(r#"(\w+)\s*=\s*\[.*"#).unwrap();

    let mut lines = cargo.lines();
    while let Some(line) = lines.next() {
        let line = line.trim();
        if line == "[features]" {
            in_features = true;
        } else if in_features {
            use std::fmt::*;

            if line.starts_with('[') && line.ends_with(']') {
                break;
            }

            if line.starts_with('#') {
                let docs = line.trim_start_matches(&['#', ' ']);
                writeln!(&mut next_docs, "{docs}").unwrap();
            } else {
                if let Some(caps) = rgx.captures(&line) {
                    let name = caps.get(1).unwrap().as_str();
                    if name == "default" {
                        let s = line.find('[').unwrap();
                        let mut defaults = String::new();
                        if let Some(e) = line.find(']') {
                            defaults.push_str(&line[s + 1..e]);
                        } else {
                            defaults.push_str(&line[s + 1..]);
                            while let Some(line) = lines.next() {
                                if let Some(e) = line.find(']') {
                                    defaults.push_str(&line[..e]);
                                    break;
                                }
                                defaults.push_str(line);
                            }
                        }
                        for dft in defaults.split(',') {
                            rd.insert(dft.trim_matches(&['"', ' ']).to_owned());
                        }
                    } else {
                        r.push(Feature {
                            name: name.to_owned(),
                            docs: std::mem::take(&mut next_docs),
                        })
                    };
                } else {
                    next_docs.clear();
                }
            }
        }
    }
    (r, rd)
}

const HEADER: &str = "This crate is part of the [`zng`](https://github.com/zng-ui/zng?tab=readme-ov-file#crates) project.\n";

const README_TEMPLATE: &str = "\
<!--do doc --readme header-->
.


<!--do doc --readme features-->


";

const FEATURES_HEADER: &str = "## Cargo Features";

const SECTION_END: &str = "<!--do doc --readme #SECTION-END-->";
