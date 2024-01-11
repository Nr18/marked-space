use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    checksum::sha256_digest,
    confluence_page::ConfluencePage,
    html::{format_document_with_plugins, LinkGenerator},
    markdown_space::MarkdownSpace,
    parent::get_parent_title,
};
use comrak::{
    nodes::{AstNode, NodeValue},
    parse_document, Arena, Options, Plugins,
};
use serde::Deserialize;

use crate::{error::ConfluenceError, Result};

#[derive(Deserialize, Debug)]
pub struct FrontMatter {
    pub labels: Vec<String>,
}

pub struct MarkdownPage<'a> {
    pub title: String,
    pub source: String,
    root: &'a AstNode<'a>,
    pub attachments: Vec<PathBuf>,
    pub local_links: Vec<PathBuf>,
    pub front_matter: Option<FrontMatter>,
}

impl<'a> MarkdownPage<'a> {
    pub fn parse(
        markdown_space: &MarkdownSpace,
        markdown_page: &Path,
        arena: &'a Arena<AstNode<'a>>,
    ) -> Result<MarkdownPage<'a>> {
        let content = match fs::read_to_string(markdown_page) {
            Ok(c) => c,
            Err(err) => {
                return Err(ConfluenceError::generic_error(format!(
                    "Failed to read file {}: {}",
                    markdown_page.display(),
                    err
                )))
            }
        };
        Self::parse_content(
            markdown_page,
            &content,
            arena,
            markdown_space
                .relative_page_path(markdown_page)?
                .display()
                .to_string(),
        )
    }

    fn options() -> Options {
        let mut options = Options::default();
        // options.extension.autolink = true;
        options.extension.table = true;
        options.extension.tasklist = true;
        // options.extension.strikethrough = true;
        options.extension.front_matter_delimiter = Some("---".to_string());
        // options.extension.shortcodes = true;
        options
    }

    fn parse_content(
        markdown_page: &Path,
        content: &str,
        arena: &'a Arena<AstNode<'a>>,
        source: String,
    ) -> Result<MarkdownPage<'a>> {
        let root: &AstNode<'_> = parse_document(arena, content, &Self::options());

        fn iter_nodes<'a, F>(node: &'a AstNode<'a>, f: &mut F)
        where
            F: FnMut(&'a AstNode<'a>),
        {
            f(node);
            for c in node.children() {
                iter_nodes(c, f);
            }
        }

        let mut errors = Vec::<String>::default();
        let mut attachments = Vec::<PathBuf>::default();
        let mut local_links = Vec::<PathBuf>::default();
        let mut first_heading: Option<&AstNode> = None;
        let mut front_matter: Option<FrontMatter> = None;
        iter_nodes(root, &mut |node| match &mut node.data.borrow_mut().value {
            NodeValue::FrontMatter(front_matter_str) => {
                let front_matter_str = front_matter_str
                    .strip_prefix("---")
                    .unwrap()
                    .strip_suffix("---\n")
                    .unwrap();
                match serde_yaml::from_str(&front_matter_str) {
                    Ok(front_matter_yaml) => {
                        front_matter = Some(front_matter_yaml);
                    }
                    Err(err) => {
                        errors.push(format!("Couldn't parse front matter: {}", err));
                    }
                }
            }
            NodeValue::Heading(_heading) => {
                if first_heading.is_none() {
                    first_heading = Some(node);
                }
            }
            NodeValue::Image(image) => {
                if !image.url.starts_with("http") {
                    let mut attachment_path = PathBuf::from(markdown_page.parent().unwrap());
                    attachment_path.push(image.url.clone());
                    attachments.push(attachment_path);
                }
            }
            NodeValue::Link(node_link) => {
                if !(node_link.url.starts_with("http://") || node_link.url.starts_with("https://"))
                {
                    let link_path = PathBuf::from(source.as_str())
                        .parent()
                        .unwrap()
                        .join(&node_link.url);
                    local_links.push(link_path);
                }
            }
            // NodeValue::TaskItem(_item) => match &node.parent().unwrap().data.borrow_mut().value {
            //     NodeValue::List(mut list) => {
            //         println!("Task list item is list");
            //         list.bullet_char = b'x';
            //     }
            //     _ => println!("Task list item is NOT list"),
            // },
            _ => (),
        });

        let mut title = String::default();

        if let Some(heading_node) = first_heading {
            for c in heading_node.children() {
                iter_nodes(c, &mut |child| {
                    if let NodeValue::Text(text) = &mut child.data.borrow_mut().value {
                        title += text
                    }
                });
            }
            // TODO: it's still allocated tho...
            heading_node.detach();
        } else {
            errors.push(String::from("missing first heading for title"));
        }

        if errors.is_empty() {
            Ok(MarkdownPage {
                title,
                source,
                root,
                attachments,
                local_links,
                front_matter,
            })
        } else {
            Err(ConfluenceError::parsing_errors(source, errors))
        }
    }

    fn to_html_string(&self, link_generator: &LinkGenerator) -> Result<String> {
        let plugins = Plugins::default();

        let mut html = vec![];
        format_document_with_plugins(
            self.root,
            &Self::options(),
            &mut html,
            &plugins,
            link_generator,
        )
        .unwrap();

        match String::from_utf8(html) {
            Ok(content) => Ok(content),
            Err(_err) => Err(ConfluenceError::generic_error("Failed to convert to utf8")),
        }
    }

    pub fn render(&self, link_generator: &LinkGenerator) -> Result<RenderedPage> {
        let content = self.to_html_string(link_generator)?.clone();
        let title = self.title.clone();
        let page_path = PathBuf::from(self.source.clone());
        let parent = get_parent_title(page_path, link_generator)?;
        let checksum = sha256_digest(content.as_bytes())?;

        Ok(RenderedPage {
            title,
            content,
            source: self.source.clone(),
            parent,
            checksum,
        })
    }
}

#[derive(Debug)]
pub struct RenderedPage {
    pub title: String,
    pub content: String,
    pub source: String,
    pub parent: Option<String>,
    pub checksum: String,
}

impl RenderedPage {
    pub fn is_home_page(&self) -> bool {
        self.source == "index.md"
    }

    pub fn version_message(&self) -> String {
        format!(
            "{} source={}; checksum={}",
            ConfluencePage::version_message_prefix(),
            self.source.replace('\\', "/"), // needs to be platform independent
            self.checksum
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use comrak::{nodes::AstNode, Arena};

    use crate::error::TestResult;
    use crate::html::LinkGenerator;
    use crate::markdown_page::MarkdownPage;

    #[test]
    fn it_get_first_heading_as_title() -> TestResult {
        let arena = Arena::<AstNode>::new();
        let page = MarkdownPage::parse_content(
            PathBuf::from("page.md").as_path(),
            &String::from("# My Page Title\n\nMy page content"),
            &arena,
            "page.md".into(),
        )?;

        assert_eq!(page.title, "My Page Title");

        Ok(())
    }

    #[test]
    fn it_removes_title_heading_and_renders_content() -> TestResult {
        let arena = Arena::<AstNode>::new();
        let page = MarkdownPage::parse_content(
            PathBuf::from("page.md").as_path(),
            &String::from("# My Page Title\n\nMy page content"),
            &arena,
            "page.md".into(),
        )?;

        let content = page.to_html_string(&LinkGenerator::new())?;

        assert!(content.contains("My page content"));
        assert!(!content.contains("<h1>My Page Title</h1>"));

        Ok(())
    }

    #[test]
    fn it_errors_if_no_heading() -> TestResult {
        let arena = Arena::<AstNode>::new();
        let page = MarkdownPage::parse_content(
            PathBuf::from("page.md").as_path(),
            &String::from("My page content"),
            &arena,
            "page.md".into(),
        );

        assert!(page.is_err());
        assert_eq!(
            page.err().unwrap().to_string(),
            "Failed to parse page.md: missing first heading for title"
        );

        Ok(())
    }

    fn _it_warns_if_title_and_filename_dont_agree() {}
    fn _it_fails_if_first_non_frontmatter_element_is_not_h1() {}

    #[test]
    fn it_translates_file_links_to_title_links() -> TestResult {
        let link_filename = PathBuf::from("hello-world.md");
        let link_file_title = String::from("This is the title parsed from the linked file");
        let link_text = String::from("Link text");
        let arena = Arena::<AstNode>::new();
        let page = MarkdownPage::parse_content(
            PathBuf::from("page.md").as_path(),
            &format!(
                "# My Page Title\n\nMy page content: [{}]({})",
                link_text,
                link_filename.display()
            ),
            &arena,
            "page.md".into(),
        )?;

        let mut link_generator = LinkGenerator::new();

        link_generator.add_file_title(&link_filename, &link_file_title)?;

        let content = page.to_html_string(&link_generator)?;

        assert!(content.contains(format!("ri:content-title=\"{}\"", link_file_title).as_str()));
        assert!(content.contains(format!("<![CDATA[{}]]>", link_text).as_str()));

        Ok(())
    }

    #[test]
    fn it_skips_unknown_local_links() {}

    #[test]
    fn it_renders_local_file_as_attached_image() -> TestResult {
        // <ac:image>
        //     <ri:attachment ri:filename="atlassian_logo.gif" />
        // </ac:image>
        let arena = Arena::<AstNode>::new();
        let page = MarkdownPage::parse_content(
            PathBuf::from("page.md").as_path(),
            "# My Page Title\n\nMy page content: ![myimage](myimage.png)",
            &arena,
            "page.md".into(),
        )?;

        assert_eq!(page.attachments.len(), 1);

        let content = page.to_html_string(&LinkGenerator::new())?;

        assert!(content.contains(r#"<ri:attachment ri:filename="myimage.png"/>"#));

        Ok(())
    }

    fn _it_raises_an_error_when_image_file_does_not_exist() {}

    #[test]
    fn it_renders_url_as_external_image() {
        // <ac:image>
        //     <ri:url ri:value="http://confluence.atlassian.com/images/logo/confluence_48_trans.png" />
        // </ac:image>
    }

    fn _it_checks_attachment_links() {}
}
