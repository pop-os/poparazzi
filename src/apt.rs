use anyhow::{Result, anyhow};
use async_compression::futures::bufread::GzipDecoder;
use deb_control_codec::{asynchronous_codec::FramedRead, prelude::*};
use futures_util::{StreamExt, TryStreamExt};
use url::Url;

fn parse_array(entry: Entry, value: &mut Option<Vec<String>>) -> Result<()> {
    if value.is_some() {
        return Err(anyhow!("entry {} already set", entry.key));
    }
    *value = Some(entry.value.split(' ').map(|x| x.to_string()).collect());
    Ok(())
}

fn parse_string(entry: Entry, value: &mut Option<String>) -> Result<()> {
    if value.is_some() {
        return Err(anyhow!("entry {} already set", entry.key));
    }
    *value = Some(entry.value.to_string());
    Ok(())
}

#[derive(Debug, Default)]
pub struct Release {
    pub archs: Option<Vec<String>>,
    pub codename: Option<String>,
    pub components: Option<Vec<String>>,
}

impl TryFrom<Control<'_>> for Release {
    type Error = anyhow::Error;
    fn try_from(control: Control) -> Result<Self> {
        let mut this = Self::default();
        for entry in control {
            match entry.key {
                "Architectures" => parse_array(entry, &mut this.archs)?,
                "Codename" => parse_string(entry, &mut this.codename)?,
                "Components" => parse_array(entry, &mut this.components)?,
                _ => {}
            }
        }
        Ok(this)
    }
}

#[derive(Debug, Default)]
pub struct Package {
    pub package: Option<String>,
    pub archs: Option<Vec<String>>,
    pub version: Option<String>,
    pub source: Option<String>,
}

impl TryFrom<Control<'_>> for Package {
    type Error = anyhow::Error;
    fn try_from(control: Control) -> Result<Self> {
        let mut this = Self::default();
        for entry in control {
            match entry.key {
                "Package" => parse_string(entry, &mut this.package)?,
                "Architectures" => parse_array(entry, &mut this.archs)?,
                "Version" => parse_string(entry, &mut this.version)?,
                "Source" => parse_string(entry, &mut this.source)?,
                _ => {}
            }
        }
        Ok(this)
    }
}

#[derive(Debug, Default)]
pub struct Source {
    pub package: Option<String>,
    pub archs: Option<Vec<String>>,
    pub version: Option<String>,
    pub directory: Option<String>,
}

impl TryFrom<Control<'_>> for Source {
    type Error = anyhow::Error;
    fn try_from(control: Control) -> Result<Self> {
        let mut this = Self::default();
        for entry in control {
            match entry.key {
                "Package" => parse_string(entry, &mut this.package)?,
                "Architectures" => parse_array(entry, &mut this.archs)?,
                "Version" => parse_string(entry, &mut this.version)?,
                "Directory" => parse_string(entry, &mut this.directory)?,
                _ => {}
            }
        }
        Ok(this)
    }
}

#[derive(Clone)]
pub struct AptRepo {
    url: Url,
}

impl AptRepo {
    pub fn new(url: Url) -> Self {
        Self { url }
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        let url = self.url.join(path)?;
        let response = reqwest::get(url).await?.error_for_status()?;
        Ok(response)
    }

    async fn get_control<T, F: Fn(Control) -> Result<T>>(
        &self,
        path: &str,
        map_control: F,
    ) -> Result<Vec<T>> {
        let response = self.get(path).await?;
        let stream = response
            .bytes_stream()
            .map_err(std::io::Error::other)
            .into_async_read();
        let mut control_stream = FramedRead::new(stream, ControlDecoder::default());
        //TODO: return mapped stream
        let mut res = Vec::new();
        while let Some(event) = control_stream.next().await {
            let event = event.unwrap();
            let event = str::from_utf8(&event).expect("not UTF8");
            res.push(map_control(Control::new(&event))?);
        }

        Ok(res)
    }

    //TODO: reduce code replication with get_control
    async fn get_control_gzip<T, F: Fn(Control) -> Result<T>>(
        &self,
        path: &str,
        map_control: F,
    ) -> Result<Vec<T>> {
        let response = self.get(path).await?;
        let stream = response.bytes_stream().map_err(std::io::Error::other);
        let stream = GzipDecoder::new(stream.into_async_read());
        let mut control_stream = FramedRead::new(stream, ControlDecoder::default());
        //TODO: return mapped stream
        let mut res = Vec::new();
        while let Some(event) = control_stream.next().await {
            let event = event.unwrap();
            let event = str::from_utf8(&event).expect("not UTF8");
            res.push(map_control(Control::new(&event))?);
        }

        Ok(res)
    }

    pub async fn release(&self, suite: &str) -> Result<Vec<Release>> {
        self.get_control(&format!("dists/{suite}/Release"), |control| {
            Release::try_from(control)
        })
        .await
    }

    pub async fn packages(&self, suite: &str, component: &str, arch: &str) -> Result<Vec<Package>> {
        self.get_control_gzip(
            &format!("dists/{suite}/{component}/binary-{arch}/Packages.gz"),
            |control| Package::try_from(control),
        )
        .await
    }

    pub async fn sources(&self, suite: &str, component: &str) -> Result<Vec<Source>> {
        self.get_control_gzip(
            &format!("dists/{suite}/{component}/source/Sources.gz"),
            |control| Source::try_from(control),
        )
        .await
    }
}
