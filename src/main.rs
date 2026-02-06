use anyhow::{Context, Result, anyhow};
use html_escape::encode_text;
use octocrab::Octocrab;
use std::{cell::RefCell, collections::BTreeMap, fmt::Write as _, fs, io::Write};

mod apt;
use self::apt::AptRepo;
mod config;
use self::config::*;

#[derive(Clone, Debug)]
pub struct AptVersion {
    repo_kind: RepoKind,
    codename: Codename,
    version: String,
    directory: Option<String>,
    errors: RefCell<Vec<String>>,
}

impl AptVersion {
    fn github_commit(&self) -> Option<String> {
        let directory = self.directory.as_ref()?;
        let mut parts = directory.split('/');
        assert_eq!(parts.next()?, "pool");
        let _codename = parts.next()?;
        let repo = parts.next()?;
        let commit = parts.next()?;
        Some(format!(
            "https://github.com/{GITHUB_ORG}/{repo}/commit/{commit}"
        ))
    }

    fn html_cell<W: Write>(&self, html: &mut W, package: &str) -> Result<()> {
        let errors = self.errors.borrow();
        if errors.is_empty() {
            writeln!(html, "<td>",)?;
        } else {
            writeln!(html, "<td class='error'>",)?;
        }
        let url_opt = match self.repo_kind {
            RepoKind::Stable => Some(format!(
                "https://launchpad.net/~system76-dev/+archive/ubuntu/stable/+packages?field.name_filter={}&field.status_filter=published&field.series_filter={}",
                urlencoding::encode(package),
                urlencoding::encode(self.codename.as_str())
            )),
            RepoKind::PreStable => Some(format!(
                "https://launchpad.net/~system76-dev/+archive/ubuntu/pre-stable/+packages?field.name_filter={}&field.status_filter=published&field.series_filter={}",
                urlencoding::encode(package),
                urlencoding::encode(self.codename.as_str())
            )),
            RepoKind::Ubuntu => Some(format!(
                "https://launchpad.net/ubuntu/+source/{}/{}",
                package, self.version
            )),
            _ => self.github_commit(),
        };
        if let Some(url) = url_opt {
            writeln!(
                html,
                "<a href='{url}'>{}</a>",
                // Allows version to line break at punctuation
                encode_text(&self.version)
                    .replace("~", "~&#8203;")
                    .replace("-", "-&#8203;")
                    .replace("+", "+&#8203;")
            )?;
        } else {
            writeln!(html, "{}", encode_text(&self.version))?;
        }
        for error in errors.iter() {
            writeln!(html, "<br/>{}", encode_text(error))?;
        }
        writeln!(html, "</td>")?;
        Ok(())
    }
}

#[derive(Clone, Debug, Default)]
pub struct AptInfo {
    release: Option<AptVersion>,
    staging: Option<AptVersion>,
    staging_ubuntu: Option<AptVersion>,
    stable: Option<AptVersion>,
    pre_stable: Option<AptVersion>,
    ubuntu: Option<AptVersion>,
}

impl AptInfo {
    pub fn version(&self, repo_kind: RepoKind) -> &Option<AptVersion> {
        match repo_kind {
            RepoKind::Release => &self.release,
            RepoKind::Staging => &self.staging,
            RepoKind::StagingUbuntu => &self.staging_ubuntu,
            RepoKind::Stable => &self.stable,
            RepoKind::PreStable => &self.pre_stable,
            RepoKind::Ubuntu => &self.ubuntu,
        }
    }

    pub fn version_mut(&mut self, repo_kind: RepoKind) -> &mut Option<AptVersion> {
        match repo_kind {
            RepoKind::Release => &mut self.release,
            RepoKind::Staging => &mut self.staging,
            RepoKind::StagingUbuntu => &mut self.staging_ubuntu,
            RepoKind::Stable => &mut self.stable,
            RepoKind::PreStable => &mut self.pre_stable,
            RepoKind::Ubuntu => &mut self.ubuntu,
        }
    }
}

// Uses a BTreeMap so it stays sorted
type AptInfos = BTreeMap<(String, Codename), AptInfo>;

async fn apt_infos() -> Result<AptInfos> {
    log::info!("fetching repository data in parallel");
    let mut release_tasks = Vec::new();
    for repo_kind in RepoKind::all() {
        let repo = AptRepo::new(repo_kind.url());
        let mut repo_tasks = Vec::new();
        for codename in repo_kind.codenames() {
            for suite in repo_kind.suites(*codename) {
                repo_tasks.push((codename, suite, {
                    let repo = repo.clone();
                    tokio::spawn(async move { repo.release(&suite.to_string()).await })
                }));
            }
        }
        release_tasks.push((repo_kind, repo_tasks));
    }

    let mut tasks = Vec::new();
    for (repo_kind, release_repo_tasks) in release_tasks {
        let repo = AptRepo::new(repo_kind.url());
        let mut repo_tasks = Vec::new();
        for (codename, suite, release_task) in release_repo_tasks {
            let mut suite_tasks = Vec::new();
            let releases = release_task.await??;
            assert_eq!(releases.len(), 1);
            for release in releases {
                for component in release
                    .components
                    .as_ref()
                    .ok_or(anyhow!("release missing components"))?
                {
                    let sources_task = {
                        let repo = repo.clone();
                        let component = component.clone();
                        tokio::spawn(
                            async move { repo.sources(&suite.to_string(), &component).await },
                        )
                    };

                    let mut arch_tasks = Vec::new();
                    for arch in release
                        .archs
                        .as_ref()
                        .ok_or(anyhow!("release missing archs"))?
                    {
                        let mut allowed = false;
                        for allowed_arch in repo_kind.allowed_archs() {
                            if arch == allowed_arch.as_str() {
                                allowed = true;
                                break;
                            }
                        }
                        if !allowed {
                            continue;
                        }

                        //TODO: use Packages data (slower)
                        if false {
                            arch_tasks.push((arch.clone(), {
                                let repo = repo.clone();
                                let component = component.clone();
                                let arch = arch.clone();
                                tokio::spawn(async move {
                                    repo.packages(&suite.to_string(), &component, &arch).await
                                })
                            }));
                        }
                    }

                    suite_tasks.push((component.clone(), sources_task, arch_tasks));
                }
            }
            repo_tasks.push((codename, suite, suite_tasks));
        }
        tasks.push((repo_kind, repo_tasks));
    }

    let mut apt_infos = AptInfos::new();
    for (repo_kind, repo_tasks) in tasks {
        println!("{:?}", repo_kind);
        for (codename, suite, suite_tasks) in repo_tasks {
            println!("\t{}", suite);
            for (component, sources_task, arch_tasks) in suite_tasks {
                let sources = sources_task.await??;
                println!("\t\t{}: {} sources", component, sources.len());
                for source in sources {
                    let Some(package) = source.package else {
                        continue;
                    };
                    let Some(version) = source.version else {
                        continue;
                    };
                    let apt_version = || AptVersion {
                        repo_kind,
                        codename: *codename,
                        version: version.clone(),
                        directory: source.directory.clone(),
                        errors: RefCell::new(Vec::new()),
                    };
                    let entry = apt_infos.entry((package, *codename));
                    match repo_kind {
                        RepoKind::Ubuntu => {
                            // Only insert Ubuntu versions if a Pop version is found
                            entry.and_modify(|apt_info| match &apt_info.ubuntu {
                                Some(last) => {
                                    if let std::cmp::Ordering::Greater =
                                        deb_version::compare_versions(&version, &last.version)
                                    {
                                        apt_info.ubuntu = Some(apt_version());
                                    }
                                }
                                None => {
                                    apt_info.ubuntu = Some(apt_version());
                                }
                            });
                        }
                        _ => {
                            let apt_info = entry.or_default();
                            let version = apt_info.version_mut(repo_kind);
                            assert!(version.is_none());
                            *version = Some(apt_version());
                        }
                    }
                }
                for (arch, packages_task) in arch_tasks {
                    let packages = packages_task.await??;
                    if !packages.is_empty() {
                        println!("\t\t{}/{}: {} packages", component, arch, packages.len());
                    }
                }
            }
        }
    }

    // Calculate errors
    for ((_package, _codename), apt_info) in apt_infos.iter() {
        for repo_kind in RepoKind::all() {
            for older_kind in repo_kind.must_be_newer_than() {
                if let Some(older_version) = apt_info.version(older_kind) {
                    if let Some(version) = apt_info.version(repo_kind) {
                        if let std::cmp::Ordering::Less =
                            deb_version::compare_versions(&version.version, &older_version.version)
                        {
                            version
                                .errors
                                .borrow_mut()
                                .push(format!("Older than {}", older_kind.as_str()));
                        }
                    } else if !matches!(older_kind, RepoKind::Ubuntu) {
                        older_version
                            .errors
                            .borrow_mut()
                            .push(format!("Not in {}", repo_kind.as_str()));
                    }
                }
            }
        }
    }

    Ok(apt_infos)
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let mut html = fs::File::create("index.html")?;
    writeln!(
        html,
        "{}",
        r#"<!DOCTYPE html>
<html lang='en'>
<head>
<meta charset='utf-8'>
<meta name='viewport' content='width=device-width'>
<title>Poparazzi</title>
<script src='https://code.jquery.com/jquery-4.0.0.min.js' integrity='sha256-OaVG6prZf4v69dPg6PhVattBXkcOWQB62pdZ3ORyrao=' crossorigin='anonymous'></script>
<link rel='stylesheet' type='text/css' href='https://cdn.datatables.net/2.3.7/css/dataTables.dataTables.min.css'>
<script type='text/javascript' src='https://cdn.datatables.net/2.3.7/js/dataTables.min.js'></script>
<style>
td.error {
    background-color: #800000
}
</style>
<script type='text/javascript'>
function onload(){
    new DataTable('#table', {
        order: [
            [0, 'desc'],
            [1, 'asc'],
            [2, 'asc']
        ],
        paging: false
    });
}
</script>
</head>
<body onload='onload()'>"#
    )?;

    writeln!(
        html,
        "<h4>Generated by <a href='https://github.com/pop-os/poparazzi'>Poparazzi</a> at {}</h4>",
        encode_text(&format!(
            "{}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z")
        ))
    )?;

    //TODO: why is this required?
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
    let token =
        fs::read_to_string(".github_token").context("Put your Github token in .github_token")?;
    let token = token.trim();
    let octocrab = Octocrab::builder().personal_token(token).build()?;

    writeln!(html, "<table width='100%'><tr>")?;
    for (name, filter) in GITHUB_PR_FILTERS {
        let filter = format!("{GITHUB_PR_FILTER_BASE} {filter}");
        let url = format!(
            "https://github.com/pulls?q={}",
            urlencoding::encode(&filter)
        );
        let page = octocrab
            .search()
            .issues_and_pull_requests(&filter)
            .send()
            .await?;
        log::info!("{name}: {}", page.total_count.unwrap_or(0));
        writeln!(
            html,
            "<td><a href='{}'>{}: {}</a></td>",
            url,
            encode_text(name),
            page.total_count.unwrap_or(0)
        )?;
        /*TODO: parse PR info?
        let stream = page
            .into_stream(&octocrab);
        pin!(stream);
        while let Some(pr) = stream.try_next().await? {
            println!(" - {}: {}", pr.html_url, pr.title);
        }
        */
    }
    writeln!(html, "</tr></table>")?;

    let apt_infos = apt_infos().await?;
    let mut total_errors = 0;
    for (_, apt_info) in apt_infos.iter() {
        for repo_kind in RepoKind::all() {
            if let Some(version) = apt_info.version(repo_kind) {
                total_errors += version.errors.borrow().len();
            }
        }
    }
    writeln!(
        html,
        "<table id='table' class='display compact' style='overflow-wrap: anywhere'>"
    )?;
    writeln!(html, "<thead>")?;
    writeln!(html, "<tr>")?;
    writeln!(html, "<th>Errors ({})</th>", total_errors)?;
    writeln!(html, "<th>Source</th>")?;
    writeln!(html, "<th>Codename</th>")?;
    for repo_kind in RepoKind::all() {
        writeln!(
            html,
            "<th><a href='{}'>{}</a></th>",
            repo_kind.url(),
            encode_text(repo_kind.as_str())
        )?;
    }
    writeln!(html, "</tr>")?;
    writeln!(html, "</thead>")?;
    writeln!(html, "<tbody>")?;
    for ((package, codename), apt_info) in apt_infos.iter() {
        writeln!(html, "<tr>")?;
        let mut errors = 0;
        for repo_kind in RepoKind::all() {
            if let Some(version) = apt_info.version(repo_kind) {
                errors += version.errors.borrow().len();
            }
        }
        if errors > 0 {
            writeln!(html, "<td class='error'>{}</td>", errors)?;
        } else {
            writeln!(html, "<td>{}</td>", errors)?;
        }
        writeln!(html, "<td>{}</td>", encode_text(&package))?;
        writeln!(html, "<td>{}</td>", encode_text(codename.as_str()))?;
        for repo_kind in RepoKind::all() {
            if let Some(version) = apt_info.version(repo_kind) {
                version.html_cell(&mut html, package)?;
            } else {
                writeln!(html, "<td>None</td>",)?;
            }
        }
        writeln!(html, "</tr>")?;
    }
    writeln!(html, "</tbody>")?;
    writeln!(html, "</table>")?;

    writeln!(
        html,
        r#"</body>
</html>"#
    )?;

    if total_errors > 0 {
        log::warn!("finished with {} errors", total_errors);
    } else {
        log::info!("finished without errors");
    }

    Ok(())
}
