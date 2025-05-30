use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use crate::confluence_paginator::ConfluencePaginator;
use crate::confluence_space::ConfluenceSpace;
use crate::{confluence_client::ConfluenceClient, responses};

use crate::error::Result;

#[derive(Debug, Clone)]
pub enum ConfluenceNodeType {
    Page(ConfluencePageData),
    Folder(ConfluenceFolder),
}

#[derive(Debug, Clone)]
pub struct ConfluenceNode {
    pub id: String,
    pub title: String,
    pub parent_id: Option<String>,
    pub data: ConfluenceNodeType,
}

impl ConfluenceNode {
    pub fn get_all(
        confluence_client: &ConfluenceClient,
        space: &ConfluenceSpace,
    ) -> Result<Vec<Self>> {
        let response = confluence_client
            .get_all_pages_in_space(&space.id)?
            .error_for_status()?;

        let mut page_iter =
            ConfluencePaginator::<responses::PageBulkWithoutBody>::new(confluence_client);

        let pages = page_iter
            .start(response)?
            .filter_map(|f| f.ok())
            .map(|bulk_page| Self::new_from_page_bulk(&bulk_page));

        let folder_response = confluence_client
            .get_all_pages_from_homepage(&space.homepage_id)?
            .error_for_status()?;

        let mut folder_iter = ConfluencePaginator::<responses::Descendant>::new(confluence_client);

        let folders = folder_iter
            .start(folder_response)?
            .filter_map(|d| d.ok())
            .filter(|d| d._type == "folder")
            .map(|d| Self::folder_from_descendant(&d));

        let result: Vec<Self> = pages.chain(folders).collect();

        Ok(result)
    }

    fn new_from_page_bulk(bulk_page: &responses::PageBulkWithoutBody) -> Self {
        Self {
            id: bulk_page.id.clone(),
            parent_id: bulk_page.parent_id.clone(),
            title: bulk_page.title.clone(),

            data: ConfluenceNodeType::Page(ConfluencePageData {
                status: bulk_page.status.clone(),
                path: ConfluencePageData::extract_path(&bulk_page.version),
                version: bulk_page.version.clone(),
            }),
        }
    }

    pub(crate) fn archive(&self, confluence_client: &ConfluenceClient) -> anyhow::Result<()> {
        let response = confluence_client
            .archive_page(&self.id, "Orphaned")?
            .error_for_status()?;

        let _body: serde_json::Value = response.json()?;
        Ok(())
    }

    pub(crate) fn unarchive(&self, confluence_client: &ConfluenceClient) -> anyhow::Result<()> {
        let response = confluence_client
            .unarchive_page(&self.id)?
            .error_for_status()?;

        let _body: serde_json::Value = response.json()?;
        Ok(())
    }

    pub(crate) fn page_data(&self) -> Option<&ConfluencePageData> {
        match &self.data {
            ConfluenceNodeType::Page(confluence_page_data) => Some(confluence_page_data),
            _ => None,
        }
    }

    fn folder_from_descendant(d: &responses::Descendant) -> Self {
        Self {
            id: d.id.clone(),
            parent_id: Some(d.parent_id.clone()),
            title: d.title.clone(),
            data: ConfluenceNodeType::Folder(ConfluenceFolder {}),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfluencePageData {
    pub version: responses::Version,
    pub path: Option<PathBuf>,
    pub status: responses::ContentStatus,
}

#[derive(Debug, Clone)]
pub struct ConfluenceFolder {}

impl ConfluencePageData {
    pub fn version_message_prefix() -> &'static str {
        "updated by markedspace:"
    }

    pub fn extract_path(version: &responses::Version) -> Option<PathBuf> {
        if let Some(data) = version
            .message
            .strip_prefix(ConfluencePageData::version_message_prefix())
        {
            let kvs: HashMap<&str, &str> = data
                .split(';')
                .map(|kv| {
                    let (key, value) = kv.split_once('=').unwrap();
                    (key.trim(), value.trim())
                })
                .collect();
            if let Some(path) = kvs.get("source") {
                PathBuf::from_str(path).ok()
            } else {
                None
            }
        } else {
            None
        }
    }

    pub(crate) fn is_managed(&self) -> bool {
        self.version
            .message
            .starts_with(ConfluencePageData::version_message_prefix())
    }
}

impl From<ConfluencePageData> for ConfluenceNodeType {
    fn from(s: ConfluencePageData) -> Self {
        ConfluenceNodeType::Page(s)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::responses;

    fn test_extract_path_from_string(s: &str) -> Option<PathBuf> {
        ConfluencePageData::extract_path(&responses::Version {
            message: String::from(s),
            number: 27,
        })
    }

    fn test_extract_path_from_string_with_prefix(s: &str) -> Option<PathBuf> {
        ConfluencePageData::extract_path(&responses::Version {
            message: ConfluencePageData::version_message_prefix().to_owned() + s,
            number: 27,
        })
    }

    #[test]
    fn it_extracts_paths() {
        let result = test_extract_path_from_string("not a markspace update");
        assert!(result.is_none());

        let result = test_extract_path_from_string_with_prefix("checksum=CHECKSUM; source=FILE");
        assert!(result.is_some());
        let path = result.unwrap();
        assert_eq!(path.as_os_str().to_str().unwrap(), "FILE");
    }
}
