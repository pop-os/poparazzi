use std::fmt;

pub const GITHUB_ORG: &'static str = "pop-os";

// Filter for all pop-os PRs that are open and not drafts
pub const GITHUB_PR_FILTER_BASE: &'static str =
    "is:open is:pr archived:false draft:false user:pop-os";
pub const GITHUB_PR_FILTERS: &'static [(&'static str, &'static str)] = &[
    (
        "PRs pending engineering assignment",
        "review:none -team-review-requested:pop-os/engineering",
    ),
    (
        "PRs pending QA assignment",
        "review:none -team-review-requested:pop-os/quality-assurance",
    ),
    (
        "PRs pending engineering review",
        "-review:changes_requested team-review-requested:pop-os/engineering",
    ),
    (
        "PRs pending QA review",
        "-review:changes_requested team-review-requested:pop-os/quality-assurance",
    ),
    ("PRs pending merge", "review:approved"),
];

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Codename {
    Jammy,
    Noble,
    Resolute,
}

impl Codename {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Jammy => "jammy",
            Self::Noble => "noble",
            Self::Resolute => "resolute",
        }
    }
}

impl fmt::Display for Codename {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SuiteKind {
    Standard,
    Security,
    Updates,
    Backports,
}

impl SuiteKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Security => "-security",
            Self::Updates => "-updates",
            Self::Backports => "-backports",
        }
    }
}

impl fmt::Display for SuiteKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Suite(Codename, SuiteKind);

impl fmt::Display for Suite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.0.as_str(), self.1.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Arch {
    Amd64,
    Arm64,
    Armhf,
    I386,
}

impl Arch {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
            Self::Armhf => "armhf",
            Self::I386 => "i386",
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RepoKind {
    Release,
    Staging,
    //TODO: ReleaseUbuntu,
    StagingUbuntu,
    Stable,
    PreStable,
    Ubuntu,
}

impl RepoKind {
    pub fn all() -> Vec<Self> {
        vec![
            Self::Release,
            Self::Staging,
            Self::StagingUbuntu,
            Self::Stable,
            Self::PreStable,
            Self::Ubuntu,
        ]
    }

    pub fn must_be_newer_than(&self) -> Vec<Self> {
        match self {
            Self::Release => vec![Self::Ubuntu],
            Self::Staging => vec![Self::Release, Self::Ubuntu],
            Self::StagingUbuntu => vec![Self::PreStable, Self::Stable, Self::Ubuntu],
            Self::Stable => vec![Self::Ubuntu],
            Self::PreStable => vec![Self::Stable, Self::Ubuntu],
            Self::Ubuntu => vec![],
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Release => "Release",
            Self::Staging => "Staging",
            Self::StagingUbuntu => "Staging (Ubuntu)",
            Self::Stable => "Stable (PPA)",
            Self::PreStable => "Pre Stable (PPA)",
            Self::Ubuntu => "Ubuntu",
        }
    }

    pub fn url(&self) -> url::Url {
        url::Url::parse(match self {
            Self::Release => "https://apt.pop-os.org/release/",
            Self::Staging => "https://apt.pop-os.org/staging/master/",
            Self::StagingUbuntu => "https://apt.pop-os.org/staging-ubuntu/master/",
            Self::Stable => "https://ppa.launchpadcontent.net/system76-dev/stable/ubuntu/",
            Self::PreStable => "https://ppa.launchpadcontent.net/system76-dev/pre-stable/ubuntu/",
            Self::Ubuntu => "https://apt.pop-os.org/ubuntu/",
        })
        .unwrap()
    }

    pub fn codenames(&self) -> &'static [Codename] {
        &[Codename::Jammy, Codename::Noble, Codename::Resolute]
    }

    pub fn suites(&self, codename: Codename) -> Vec<Suite> {
        match self {
            Self::Ubuntu => {
                vec![
                    Suite(codename, SuiteKind::Standard),
                    Suite(codename, SuiteKind::Security),
                    Suite(codename, SuiteKind::Updates),
                    Suite(codename, SuiteKind::Backports),
                ]
            }
            _ => {
                vec![Suite(codename, SuiteKind::Standard)]
            }
        }
    }

    pub fn allowed_archs(&self) -> &'static [Arch] {
        match self {
            Self::Ubuntu => &[Arch::Amd64, Arch::I386],
            _ => &[Arch::Amd64, Arch::Arm64, Arch::Armhf, Arch::I386],
        }
    }
}
