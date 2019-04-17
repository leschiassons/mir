use git2::{build::RepoBuilder, Cred, CredentialType, Error, FetchOptions, RemoteCallbacks};
use gitlab::Gitlab;
use indicatif::{ProgressBar, ProgressStyle};
use num;
use rpassword;
use structopt::StructOpt;

use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// Tool to mirror a user's entire owned GitLab group hierarchy locally and optionally clone all projects
#[derive(StructOpt, Debug)]
#[structopt(name = "mir")]
struct CliArgs {
    /**
     * Access level of groups (and projects if --clone flag provided)
     * -A     => Guest Access [default]
     * -AA    => Reporter Access
     * -AAA   => Developer Access
     * -AAAA  => Maintainer Access
     * -AAAAA => Owner Access
     */
    #[structopt(short = "A", long = "access-level", parse(from_occurrences))]
    access_level: u8,
    /// Clone all repositories
    #[structopt(short = "c", long = "clone")]
    clone: bool,
    /// The destination directory in which the hierarchy should be mirrored
    #[structopt(short = "d", long = "destination", default_value = ".")]
    destination: String,
    /// GitLab remote host
    #[structopt(short = "H", long = "host", default_value = "gitlab.com")]
    host: String,
    /// GitLab personal access token
    #[structopt(short = "p", long = "personal-access-token")]
    personal_access_token: Option<String>,
    // /// Verbose mode (-v, -vv, -vvv, etc.)
    // #[structopt(short = "v", long = "verbose", parse(from_occurrences))]
    // verbose: u8,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = CliArgs::from_args();
    let access_level = format!("{}", num::clamp(args.access_level, 1, 5) * 10);
    let password = match args.personal_access_token {
        Some(p) => p,
        None => rpassword::prompt_password_stdout("Enter GitLab personal access token: ")?
    };
    let gitlab = Gitlab::new(&args.host, &password)?;
    let user = gitlab.current_user()?;
    let groups = gitlab.groups(&[("min_access_level", &access_level)])?;
    let projects = gitlab.projects(&[("min_access_level", &access_level)])?;
    let mut namespaces = groups.iter().map(|ref g| &g.full_path).collect::<HashSet<_>>();
    namespaces.insert(&user.username);
    for n in namespaces {
        let namespace = format!("{}/{}", args.destination, n);
        println!("mkdir '{}'", namespace);
        fs::create_dir_all(&namespace)?;
    }
    if args.clone {
        let progress_bar = ProgressBar::new(0);
        progress_bar.set_style(ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {msg:18}: {bar:40.magenta/red} {pos}/{len}")
            .progress_chars("##-"));
        let mut remote_callbacks = RemoteCallbacks::new();
        remote_callbacks
            .credentials(|_, _, credential_type| {
                match credential_type {
                    CredentialType::USER_PASS_PLAINTEXT => Cred::userpass_plaintext(&user.username, &password),
                    _ => Err(Error::from_str(&format!("Unsupported requested credential type '{:?}'", credential_type))),
                }
            })
            .transfer_progress(|p| {
                if p.received_objects() < p.total_objects() {
                    progress_bar.set_message("Receiving objects");
                    progress_bar.set_length(p.total_objects() as u64);
                    progress_bar.set_position(p.received_objects() as u64);
                } else {
                    progress_bar.set_message("Resolving deltas");
                    progress_bar.set_length(p.total_deltas() as u64);
                    progress_bar.set_position(p.indexed_deltas() as u64);
                }
                true
            });
        let mut fetch_options = FetchOptions::new();
        fetch_options.remote_callbacks(remote_callbacks);
        let mut repo_builder = RepoBuilder::new();
        repo_builder.fetch_options(fetch_options);
        for p in projects {
            let namespace = format!("{}/{}/{}", args.destination, p.namespace.full_path, p.name);
            progress_bar.println(format!("Cloning into '{}'...", namespace));
            fs::create_dir_all(&namespace).expect(&format!("failed to create the directory '{}'", namespace));
            repo_builder.clone(&p.http_url_to_repo, Path::new(&namespace))?;
            progress_bar.finish();
        }
        progress_bar.finish_and_clear();
    }
    Ok(())
}
