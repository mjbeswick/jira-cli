use crate::config::Profile;
use anyhow::{bail, Context, Result};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use reqwest::blocking::{Client, RequestBuilder};
use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::HashSet;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use url::Url;

pub struct Jira {
    http: Client,
    config: Profile,
    version: Cell<u8>,
}

#[derive(Debug)]
struct IncompatibleResponse(String);

impl fmt::Display for IncompatibleResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for IncompatibleResponse {}

impl Jira {
    pub fn new(config: Profile) -> Result<Self> {
        // Jira is the only permitted destination. Never forward credentials or
        // cookies to a redirect target (including an SSO login page).
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none());
        if config.auth_type == "mtls" {
            let certificate =
                std::fs::read(&config.tls_client_cert).context("Cannot read client certificate")?;
            let key = std::fs::read(&config.tls_client_key).context("Cannot read client key")?;
            builder = builder.identity(
                reqwest::Identity::from_pkcs8_pem(&certificate, &key)
                    .context("Invalid client identity (PKCS#8 PEM key required)")?,
            );
            if !config.tls_ca_cert.is_empty() {
                let pem =
                    std::fs::read(&config.tls_ca_cert).context("Cannot read CA certificate")?;
                builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&pem)?);
            }
        }
        let version = if config.api_version == "2" { 2 } else { 3 };
        Ok(Self {
            http: builder.build()?,
            config,
            version: Cell::new(version),
        })
    }

    pub fn server(&self) -> &str {
        &self.config.server
    }

    fn root(&self) -> String {
        if self.config.cloud_id.is_empty() {
            self.config.server.clone()
        } else {
            format!("https://api.atlassian.com/ex/jira/{}", self.config.cloud_id)
        }
    }

    fn url(&self, path: &str, agile: bool, version: u8) -> String {
        let root = self.root();
        if agile {
            format!("{root}/rest/agile/1.0{path}")
        } else {
            format!("{root}/rest/api/{version}{path}")
        }
    }

    fn service_desk_comment_url(&self, key: &str) -> String {
        format!(
            "{}/rest/servicedeskapi/request/{}/comment",
            self.root(),
            encoded(key)
        )
    }

    fn auth(&self, request: RequestBuilder) -> RequestBuilder {
        let request = match self.config.auth_type.as_str() {
            "basic" => request.basic_auth(&self.config.username, Some(&self.config.token)),
            "bearer" => request.bearer_auth(&self.config.token),
            _ => request,
        };
        if self.config.cookie.is_empty() {
            request
        } else {
            request.header(reqwest::header::COOKIE, &self.config.cookie)
        }
    }

    fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
        version: u8,
        agile: bool,
    ) -> Result<Value> {
        let url = self.url(path, agile, version);
        self.send_to(method, url, body)
    }

    fn send_to(&self, method: reqwest::Method, url: String, body: Option<&Value>) -> Result<Value> {
        let request = self.http.request(method, url);
        let request = if let Some(payload) = body {
            request.json(payload)
        } else {
            request
        };
        let response = self
            .auth(request)
            .send()
            .context("Jira request failed; if this was a write, check Jira before retrying")?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = response.text().context("Cannot read Jira response")?;
        if !status.is_success() {
            let detail = serde_json::from_str::<Value>(&text)
                .ok()
                .map(|value| jira_error_detail(&value))
                .filter(|detail| !detail.is_empty())
                .unwrap_or_else(|| format!("HTTP {status}"));
            let message = format!("Jira returned HTTP {status}: {detail}");
            if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
                return Err(IncompatibleResponse(message).into());
            }
            bail!("{message}");
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let media_type = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if media_type != "application/json"
            && !(media_type.starts_with("application/") && media_type.ends_with("+json"))
        {
            return Err(IncompatibleResponse("Jira did not return JSON; check authentication and whether the operation completed before retrying".into()).into());
        }
        serde_json::from_str(&text).map_err(|error| {
            IncompatibleResponse(format!("Jira returned invalid JSON: {error}")).into()
        })
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        self.get_with_fallback(path, path)
    }

    fn get_with_fallback(&self, path_v3: &str, path_v2: &str) -> Result<Value> {
        let version = self.version.get();
        let path = if version == 2 { path_v2 } else { path_v3 };
        match self.send(reqwest::Method::GET, path, None, version, false) {
            Err(error)
                if version == 3
                    && self.config.api_version == "auto"
                    && error.is::<IncompatibleResponse>() =>
            {
                let result = self.send(reqwest::Method::GET, path_v2, None, 2, false)?;
                self.version.set(2);
                Ok(result)
            }
            result => result,
        }
    }
    pub fn get_agile(&self, path: &str) -> Result<Value> {
        self.send(reqwest::Method::GET, path, None, self.version.get(), true)
    }
    pub fn write(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value> {
        self.send(method, path, body, self.version.get(), false)
    }

    /// List attachment metadata (id, filename, mimeType, size, author, created,
    /// and an absolute `content` download URL) for an issue, including
    /// attachments referenced inline from a rich-text description or comment.
    /// Rendering those fields as Markdown cannot embed binary image data, so
    /// inline media nodes render as the literal text "[media]"; this is how an
    /// agent discovers and retrieves what those placeholders point to.
    pub fn get_attachments(&self, issue_key: &str) -> Result<Value> {
        let response = self.get(&format!("/issue/{}?fields=attachment", encoded(issue_key)))?;
        Ok(response["fields"]["attachment"]
            .as_array()
            .cloned()
            .map(Value::Array)
            .unwrap_or_else(|| Value::Array(Vec::new())))
    }

    /// Download one attachment's bytes to `dest_path` using its own absolute
    /// `content` URL. The initial URL must match the configured Jira origin.
    /// Jira may respond with a 303 to a signed media URL; redirects are followed
    /// without Jira credentials or cookies, and HTTPS downgrades are refused.
    pub fn download_attachment(&self, content_url: &str, dest_path: &Path) -> Result<()> {
        let mut target = Url::parse(content_url).context("Attachment content URL is not valid")?;
        let allowed = Url::parse(&self.root()).context("Configured Jira server URL is invalid")?;
        if target.scheme() != "https" && target.scheme() != "http" {
            bail!("Refusing to download from a non-HTTP(S) attachment URL");
        }
        if target.host_str() != allowed.host_str()
            || target.port_or_known_default() != allowed.port_or_known_default()
        {
            bail!(
                "Refusing to download from attachment URL host {:?}; expected {:?}",
                target.host_str(),
                allowed.host_str()
            );
        }
        let mut send_jira_credentials = true;
        let mut redirects_followed = 0;
        let response = loop {
            let request = self.http.get(target.clone());
            let response = if send_jira_credentials {
                self.auth(request).send()
            } else {
                request.send()
            }
            .context("Attachment download request failed")?;
            let status = response.status();
            if status.is_redirection() {
                if redirects_followed >= 5 {
                    bail!("Attachment download exceeded the redirect limit");
                }
                if !matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308) {
                    bail!(
                        "Jira returned unsupported HTTP {status} while downloading the attachment"
                    );
                }
                let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
                    bail!("Jira redirected the attachment without a Location header");
                };
                let location = location
                    .to_str()
                    .context("Jira returned an invalid attachment redirect")?;
                let next = target
                    .join(location)
                    .context("Jira returned an invalid attachment redirect URL")?;
                if next.scheme() != "https" && next.scheme() != "http" {
                    bail!("Refusing to follow a non-HTTP(S) attachment redirect");
                }
                if target.scheme() == "https" && next.scheme() != "https" {
                    bail!("Refusing to follow an attachment redirect that downgrades HTTPS");
                }
                if !next.username().is_empty() || next.password().is_some() {
                    bail!("Refusing to follow an attachment redirect containing URL credentials");
                }
                if !send_jira_credentials && next == target {
                    bail!("Jira returned a self-referencing attachment redirect");
                }
                // Never send Jira credentials/cookies to a redirect target,
                // including same-origin redirects. CDN URLs are commonly signed.
                send_jira_credentials = false;
                redirects_followed += 1;
                target = next;
                continue;
            }
            if !status.is_success() {
                bail!("Jira returned HTTP {status} while downloading the attachment");
            }
            break response;
        };
        let bytes = response
            .bytes()
            .context("Cannot read attachment response body")?;
        let mut file = std::fs::File::create(dest_path)
            .with_context(|| format!("Cannot create {}", dest_path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("Cannot write {}", dest_path.display()))?;
        Ok(())
    }

    /// JSM Cloud's request-comment API, which explicitly controls customer visibility.
    /// Never retry or fall back to Jira's ordinary issue-comment API on failure.
    pub fn add_internal_comment(&self, key: &str, text: &str) -> Result<Value> {
        let response = self.send_to(
            reqwest::Method::POST,
            self.service_desk_comment_url(key),
            Some(&json!({"body": text, "public": false})),
        )?;
        if response["public"] != false {
            bail!("Jira did not confirm public: false for the internal comment; check the issue before retrying");
        }
        Ok(response)
    }

    /// Fetch up to `limit` issues, following the pagination scheme of the selected API.
    pub fn search_issues(&mut self, jql: &str, limit: u32) -> Result<Value> {
        if limit == 0 {
            return Ok(json!({"issues": []}));
        }
        let mut issues = Vec::new();
        let mut first_page = None;
        let mut next_token: Option<String> = None;
        let mut seen_tokens = HashSet::new();
        loop {
            let remaining = limit as usize - issues.len();
            let page_size = remaining.min(100);
            let previous_version = self.version.get();
            let v3 = format!(
                "/search/jql?jql={}&maxResults={page_size}{}",
                encoded(jql),
                next_token
                    .as_ref()
                    .map(|token| format!("&nextPageToken={}", encoded(token)))
                    .unwrap_or_default()
            );
            let v2 = format!(
                "/search?jql={}&startAt={}&maxResults={page_size}",
                encoded(jql),
                if previous_version == 3 {
                    0
                } else {
                    issues.len()
                }
            );
            let page = self.get_with_fallback(&v3, &v2)?;
            if previous_version == 3 && self.version.get() == 2 {
                // Offset pagination cannot safely continue a token-based result set.
                let had_v3_issues = !issues.is_empty();
                issues.clear();
                first_page = None;
                next_token = None;
                if had_v3_issues && page_size < limit as usize {
                    // The fallback request was sized for the *remaining* v3
                    // results. Fetch v2 from offset zero with the full limit.
                    continue;
                }
            }
            let items = page["issues"]
                .as_array()
                .context("Invalid issue search response: missing issues")?;
            if items.is_empty()
                && ((self.version.get() == 2
                    && page["total"]
                        .as_u64()
                        .is_some_and(|total| (issues.len() as u64) < total))
                    || (self.version.get() == 3 && page["nextPageToken"].is_string()))
            {
                bail!("Incomplete issue search from Jira: pagination returned no issues");
            }
            if first_page.is_none() {
                first_page = Some(page.clone());
            }
            issues.extend(items.iter().take(remaining).cloned());
            if issues.len() >= limit as usize || page["isLast"] == true {
                break;
            }
            if self.version.get() == 2 {
                if let Some(total) = page["total"].as_u64() {
                    if issues.len() as u64 >= total {
                        break;
                    }
                } else if items.len() < page_size {
                    break;
                }
            } else {
                let Some(token) = page["nextPageToken"].as_str() else {
                    break;
                };
                if !seen_tokens.insert(token.to_string()) {
                    bail!("Incomplete issue search from Jira: pagination did not advance");
                }
                next_token = Some(token.to_string());
            }
        }
        let mut result = first_page.context("Issue search returned no pages")?;
        result["issues"] = json!(issues);
        // A paginated result is a bounded list, not another page to be resumed.
        if let Some(object) = result.as_object_mut() {
            object.remove("nextPageToken");
        }
        Ok(result)
    }

    pub fn create_fields(&mut self, project: &str, issue_type: &str) -> Result<Value> {
        let project = encoded(project);
        if self.version.get() == 2 {
            return self.legacy_fields(&project, issue_type);
        }
        match self.modern_fields(&project, issue_type) {
            Err(error)
                if self.config.api_version == "auto" && error.is::<IncompatibleResponse>() =>
            {
                let fields = self.legacy_fields(&project, issue_type)?;
                self.version.set(2);
                Ok(fields)
            }
            result => result,
        }
    }

    fn modern_fields(&self, project: &str, issue_type: &str) -> Result<Value> {
        let base = format!("/issue/createmeta/{project}/issuetypes");
        let types = self.pages(&base, "issueTypes")?;
        let matches: Vec<_> = types
            .iter()
            .filter(|item| {
                item["id"] == issue_type
                    || item["name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(issue_type))
            })
            .collect();
        if matches.len() != 1 {
            bail!("Issue type {issue_type} is not uniquely available for project {project}");
        }
        let type_id = matches[0]["id"]
            .as_str()
            .context("Issue type ID is missing")?;
        let type_name = matches[0]["name"].as_str().unwrap_or(issue_type);
        let mut fields = self.pages(&format!("{base}/{}", encoded(type_id)), "fields")?;
        let needs_schema = fields.iter().any(|field| {
            field["fieldId"]
                .as_str()
                .is_some_and(|id| id.starts_with("customfield_"))
                && field["schema"].is_null()
        });
        let definitions = if needs_schema {
            Some(self.send(reqwest::Method::GET, "/field", None, 3, false)?)
        } else {
            None
        };
        for field in &mut fields {
            let id = field["fieldId"]
                .as_str()
                .or(field["key"].as_str())
                .map(str::to_owned)
                .context("Create field has no ID")?;
            if field["schema"].is_null() {
                if let Some(schema) = definitions
                    .as_ref()
                    .and_then(Value::as_array)
                    .and_then(|items| items.iter().find(|item| item["id"] == id))
                    .map(|item| &item["schema"])
                {
                    field["schema"] = schema.clone();
                }
            }
            field["id"] = json!(id);
        }
        Ok(
            json!({"project": project, "issueType": {"id": type_id, "name": type_name}, "fields": fields}),
        )
    }

    fn pages(&self, base: &str, key: &str) -> Result<Vec<Value>> {
        let mut values = Vec::new();
        loop {
            let page = self.send(
                reqwest::Method::GET,
                &format!("{base}?startAt={}&maxResults=100", values.len()),
                None,
                3,
                false,
            )?;
            let items = page[key]
                .as_array()
                .with_context(|| format!("Invalid create metadata: missing {key}"))?;
            if items.is_empty()
                && page["total"]
                    .as_u64()
                    .is_some_and(|total| values.len() < total as usize)
            {
                bail!("Incomplete create metadata from Jira");
            }
            values.extend(items.iter().cloned());
            if page["isLast"] == true
                || page["total"]
                    .as_u64()
                    .is_some_and(|total| values.len() >= total as usize)
            {
                return Ok(values);
            }
            if items.is_empty() || page["total"].is_null() {
                bail!("Incomplete create metadata from Jira");
            }
        }
    }

    fn legacy_fields(&self, project: &str, issue_type: &str) -> Result<Value> {
        let result = self.send(
            reqwest::Method::GET,
            &format!("/issue/createmeta?projectKeys={project}&expand=projects.issuetypes.fields"),
            None,
            2,
            false,
        )?;
        let projects = result["projects"]
            .as_array()
            .context("Invalid legacy create metadata")?;
        let matches: Vec<_> = projects
            .iter()
            .filter(|item| {
                item["key"]
                    .as_str()
                    .is_some_and(|key| key.eq_ignore_ascii_case(project))
            })
            .flat_map(|item| item["issuetypes"].as_array().into_iter().flatten())
            .filter(|item| {
                item["id"] == issue_type
                    || item["name"]
                        .as_str()
                        .is_some_and(|name| name.eq_ignore_ascii_case(issue_type))
            })
            .collect();
        if matches.len() != 1 {
            bail!("Issue type {issue_type} is not uniquely available for project {project}");
        }
        let mut fields = matches[0]["fields"]
            .as_object()
            .context("Create fields missing from Jira response")?
            .iter()
            .map(|(id, value)| {
                let mut field = value.clone();
                field["id"] = json!(id);
                field
            })
            .collect::<Vec<_>>();
        if fields.iter().any(|field| {
            field["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("customfield_"))
                && field["schema"].is_null()
        }) {
            let definitions = self.send(reqwest::Method::GET, "/field", None, 2, false)?;
            let definitions = definitions
                .as_array()
                .context("Invalid field schema response")?;
            for field in &mut fields {
                if field["schema"].is_null() {
                    if let Some(definition) =
                        definitions.iter().find(|item| item["id"] == field["id"])
                    {
                        field["schema"] = definition["schema"].clone();
                    }
                }
            }
        }
        Ok(
            json!({"project": project, "issueType": {"id": matches[0]["id"], "name": matches[0]["name"]}, "fields": fields}),
        )
    }

    pub fn create(&self, data: &Value) -> Result<Value> {
        let result = self.write(reqwest::Method::POST, "/issue", Some(data))?;
        if !result["key"].is_string() {
            bail!("Jira did not confirm creation with an issue key; search for the issue before retrying");
        }
        Ok(result)
    }

    pub fn version(&self) -> u8 {
        self.version.get()
    }
}

fn jira_error_detail(value: &Value) -> String {
    let mut details = value["errorMessages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();

    if let Some(errors) = value["errors"].as_object() {
        details.extend(
            errors
                .iter()
                .map(|(field, message)| format!("{field}: {message}")),
        );
    }
    if !details.is_empty() {
        return details.join(", ");
    }

    ["message", "detail", "error", "code"]
        .iter()
        .filter_map(|key| value[*key].as_str().map(|message| (*key, message)))
        .map(|(key, message)| {
            if key == "message" {
                message.to_owned()
            } else {
                format!("{key}: {message}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn encoded(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

pub fn markdown_to_adf(markdown: &str) -> Value {
    let mut content = Vec::new();
    let mut containers: Vec<Value> = Vec::new();
    let mut marks: Vec<Value> = Vec::new();
    let mut table_header = false;

    let append = |node: Value, containers: &mut Vec<Value>, content: &mut Vec<Value>| {
        if let Some(parent) = containers.last_mut() {
            parent["content"].as_array_mut().unwrap().push(node);
        } else {
            content.push(node);
        }
    };

    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES,
    ) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => containers.push(json!({"type": "paragraph", "content": []})),
                Tag::Heading { level, .. } => containers.push(json!({
                    "type": "heading",
                    "attrs": {"level": heading_level(level)},
                    "content": []
                })),
                Tag::BlockQuote(_) => containers.push(json!({"type": "blockquote", "content": []})),
                Tag::CodeBlock(kind) => {
                    let language = match kind {
                        CodeBlockKind::Fenced(info) => {
                            info.split_whitespace().next().unwrap_or("").to_string()
                        }
                        CodeBlockKind::Indented => String::new(),
                    };
                    let mut node = json!({"type": "codeBlock", "content": []});
                    if !language.is_empty() {
                        node["attrs"] = json!({"language": language});
                    }
                    containers.push(node);
                }
                Tag::List(start) => {
                    let mut node = if let Some(start) = start {
                        json!({"type": "orderedList", "attrs": {"order": start}, "content": []})
                    } else {
                        json!({"type": "bulletList", "content": []})
                    };
                    if node["type"] == "orderedList" && node["attrs"]["order"].is_null() {
                        node["attrs"]["order"] = json!(1);
                    }
                    containers.push(node);
                }
                Tag::Item => containers.push(json!({"type": "listItem", "content": []})),
                Tag::Table(_) => containers.push(json!({
                    "type": "table",
                    "attrs": {"isNumberColumnEnabled": false, "layout": "default"},
                    "content": []
                })),
                Tag::TableHead => {
                    table_header = true;
                    containers.push(json!({"type": "tableRow", "content": []}));
                }
                Tag::TableRow => {
                    table_header = false;
                    containers.push(json!({"type": "tableRow", "content": []}));
                }
                Tag::TableCell => {
                    containers.push(json!({"type": if table_header { "tableHeader" } else { "tableCell" }, "content": []}));
                    containers.push(json!({"type": "paragraph", "content": []}));
                }
                Tag::Emphasis => marks.push(json!({"type": "em"})),
                Tag::Strong => marks.push(json!({"type": "strong"})),
                Tag::Strikethrough => marks.push(json!({"type": "strike"})),
                Tag::Link { dest_url, .. } => {
                    marks.push(json!({"type": "link", "attrs": {"href": dest_url.to_string()}}));
                }
                Tag::Image { dest_url, .. } => {
                    marks.push(json!({"type": "link", "attrs": {"href": dest_url.to_string()}}));
                }
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::BlockQuote(_)
                | TagEnd::CodeBlock
                | TagEnd::List(_)
                | TagEnd::Item
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::TableCell => {
                    if let Some(node) = containers.pop() {
                        append(node, &mut containers, &mut content);
                    }
                    if tag == TagEnd::TableCell {
                        if let Some(node) = containers.pop() {
                            append(node, &mut containers, &mut content);
                        }
                    }
                }
                TagEnd::Table => {
                    if let Some(node) = containers.pop() {
                        append(node, &mut containers, &mut content);
                    }
                }
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image => {
                    marks.pop();
                }
                _ => {}
            },
            Event::Text(text) => {
                if !text.is_empty() {
                    let mut node = json!({"type": "text", "text": text.to_string()});
                    if !marks.is_empty() {
                        node["marks"] = json!(marks);
                    }
                    append(node, &mut containers, &mut content);
                }
            }
            Event::Code(text) => {
                if !text.is_empty() {
                    let mut text_marks = marks.clone();
                    text_marks.push(json!({"type": "code"}));
                    append(
                        json!({"type": "text", "text": text.to_string(), "marks": text_marks}),
                        &mut containers,
                        &mut content,
                    );
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                append(json!({"type": "hardBreak"}), &mut containers, &mut content);
            }
            Event::Rule => append(json!({"type": "rule"}), &mut containers, &mut content),
            Event::Html(html) | Event::InlineHtml(html) => {
                append(
                    json!({"type": "text", "text": html.to_string()}),
                    &mut containers,
                    &mut content,
                );
            }
            Event::TaskListMarker(checked) => append(
                json!({"type": "text", "text": if checked { "☑ " } else { "☐ " }}),
                &mut containers,
                &mut content,
            ),
            Event::FootnoteReference(label) => append(
                json!({"type": "text", "text": format!("[^{label}]")}),
                &mut containers,
                &mut content,
            ),
            Event::InlineMath(math) | Event::DisplayMath(math) => append(
                json!({"type": "text", "text": math.to_string()}),
                &mut containers,
                &mut content,
            ),
        }
    }
    while let Some(node) = containers.pop() {
        append(node, &mut containers, &mut content);
    }
    json!({"type": "doc", "version": 1, "content": content})
}

pub fn markdown_to_plain_text(markdown: &str) -> String {
    let mut output = String::new();
    let mut links = Vec::new();
    let mut ordered_lists = Vec::<u64>::new();
    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Start(tag) => match tag {
                Tag::Link { dest_url, .. } => links.push(dest_url.to_string()),
                Tag::List(start) => ordered_lists.push(start.unwrap_or(0)),
                Tag::Item => {
                    if let Some(number) = ordered_lists.last_mut() {
                        if *number > 0 {
                            output.push_str(&format!("{number}. "));
                            *number += 1;
                        } else {
                            output.push_str("- ");
                        }
                    } else {
                        output.push_str("- ");
                    }
                }
                Tag::BlockQuote(_) => output.push_str("> "),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock => {
                    output.push_str("\n\n");
                }
                TagEnd::Item => {
                    if !output.ends_with('\n') {
                        output.push('\n');
                    }
                }
                TagEnd::List(_) => {
                    ordered_lists.pop();
                    if !output.ends_with('\n') {
                        output.push('\n');
                    }
                }
                TagEnd::Link => {
                    if let Some(url) = links.pop() {
                        if !url.is_empty() {
                            output.push_str(" (");
                            output.push_str(&url);
                            output.push(')');
                        }
                    }
                }
                _ => {}
            },
            Event::Text(text) | Event::Code(text) => output.push_str(&text),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Rule => output.push_str("\n---\n"),
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "[x] " } else { "[ ] " })
            }
            Event::FootnoteReference(label) => {
                output.push_str(&format!("[^{label}]"));
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => output.push_str(&math),
            Event::Html(_) | Event::InlineHtml(_) => {}
        }
    }
    output.trim().to_string()
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    fn has_adf_node_type(value: &Value, expected: &str) -> bool {
        value["type"].as_str() == Some(expected)
            || value
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|nodes| nodes.iter().any(|node| has_adf_node_type(node, expected)))
    }

    fn has_adf_mark(value: &Value, expected: &str) -> bool {
        value["marks"]
            .as_array()
            .is_some_and(|marks| marks.iter().any(|mark| mark["type"] == expected))
            || value
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|nodes| nodes.iter().any(|node| has_adf_mark(node, expected)))
    }

    struct Reply {
        request: &'static str,
        status: &'static str,
        content_type: &'static str,
        body: String,
        location: Option<String>,
    }

    struct Captured {
        request: String,
        payload: String,
        basic_auth: bool,
        cookie: bool,
        json_content_type: bool,
    }

    fn fixture(replies: Vec<Reply>) -> (Jira, thread::JoinHandle<Vec<Captured>>) {
        fixture_with_cookie(replies, "")
    }

    fn fixture_with_cookie(
        replies: Vec<Reply>,
        cookie: &str,
    ) -> (Jira, thread::JoinHandle<Vec<Captured>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut captured = Vec::new();
            for reply in replies {
                let (stream, _) = listener.accept().unwrap();
                let request = read_request(stream, &reply);
                assert_eq!(request.request, reply.request);
                captured.push(request);
            }
            // A stray replay should be visible even if the client doesn't await its response.
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err(), "unexpected extra Jira request");
            captured
        });
        let config: Profile = serde_json::from_value(json!({
            "server": format!("http://{address}"),
            "apiVersion": "auto",
            "authType": "basic",
            "username": "user",
            "token": "secret",
            "cookie": cookie
        }))
        .unwrap();
        assert_eq!(config.api_version, "auto");
        (Jira::new(config).unwrap(), handle)
    }

    fn read_request(stream: TcpStream, reply: &Reply) -> Captured {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut first_line = String::new();
        reader.read_line(&mut first_line).unwrap();
        let mut length = 0;
        let mut basic_auth = false;
        let mut cookie = false;
        let mut json_content_type = false;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-length:") {
                length = line.split(':').nth(1).unwrap().trim().parse().unwrap();
            }
            basic_auth |= lower.starts_with("authorization: basic ");
            cookie |= lower.starts_with("cookie: session=test-cookie");
            json_content_type |= lower.starts_with("content-type: application/json");
        }
        let mut payload = vec![0; length];
        reader.read_exact(&mut payload).unwrap();
        let location = reply
            .location
            .as_ref()
            .map(|value| format!("Location: {value}\r\n"))
            .unwrap_or_default();
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: {}\r\n{location}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            reply.status,
            reply.content_type,
            reply.body.len(),
            reply.body
        );
        reader.get_mut().write_all(response.as_bytes()).unwrap();
        Captured {
            request: first_line.trim_end().to_string(),
            payload: String::from_utf8(payload).unwrap(),
            basic_auth,
            cookie,
            json_content_type,
        }
    }

    fn reply(request: &'static str, status: &'static str, body: &'static str) -> Reply {
        Reply {
            request,
            status,
            content_type: "application/json",
            body: body.to_string(),
            location: None,
        }
    }

    #[test]
    fn auto_read_falls_back_and_sticks_on_404() {
        let (client, server) = fixture(vec![
            reply(
                "GET /rest/api/3/issue/ABC-1 HTTP/1.1",
                "404 Not Found",
                "{}",
            ),
            reply(
                "GET /rest/api/2/issue/ABC-1 HTTP/1.1",
                "200 OK",
                "{\"key\":\"ABC-1\"}",
            ),
            reply("GET /rest/api/2/field HTTP/1.1", "200 OK", "[]"),
        ]);
        assert_eq!(client.get("/issue/ABC-1").unwrap()["key"], "ABC-1");
        assert_eq!(client.version(), 2);
        assert_eq!(client.get("/field").unwrap(), json!([]));
        server.join().unwrap();
    }

    #[test]
    fn auto_read_falls_back_on_gone_or_non_json() {
        for (status, content_type, body) in [
            ("410 Gone", "application/json", "{}"),
            ("200 OK", "text/html", "<html>login</html>"),
            ("200 OK", "application/json", "not json"),
        ] {
            let (client, server) = fixture(vec![
                Reply {
                    request: "GET /rest/api/3/field HTTP/1.1",
                    status,
                    content_type,
                    body: body.to_string(),
                    location: None,
                },
                reply("GET /rest/api/2/field HTTP/1.1", "200 OK", "[]"),
            ]);
            assert_eq!(client.get("/field").unwrap(), json!([]));
            assert_eq!(client.version(), 2);
            server.join().unwrap();
        }
    }

    #[test]
    fn cloud_search_uses_tokens_and_caps_results() {
        let (mut client, server) = fixture(vec![
            Reply { request: "GET /rest/api/3/search/jql?jql=project%3DABC&maxResults=100 HTTP/1.1", status: "200 OK", content_type: "application/json", body: format!("{{\"issues\":{},\"nextPageToken\":\"a/b\"}}", json!(vec![json!({"id": 1}); 100])), location: None },
            reply("GET /rest/api/3/search/jql?jql=project%3DABC&maxResults=1&nextPageToken=a%2Fb HTTP/1.1", "200 OK", "{\"issues\":[{\"id\":2}],\"isLast\":true}"),
        ]);
        let result = client.search_issues("project=ABC", 101).unwrap();
        assert_eq!(result["issues"].as_array().unwrap().len(), 101);
        assert!(result.get("nextPageToken").is_none());
        assert_eq!(client.version(), 3);
        server.join().unwrap();
    }

    #[test]
    fn search_falls_back_to_v2_and_uses_start_at() {
        let (mut client, server) = fixture(vec![
            reply("GET /rest/api/3/search/jql?jql=text%20~%20%22a%22&maxResults=100 HTTP/1.1", "404 Not Found", "{}"),
            Reply { request: "GET /rest/api/2/search?jql=text%20~%20%22a%22&startAt=0&maxResults=100 HTTP/1.1", status: "200 OK", content_type: "application/json", body: format!("{{\"issues\":{},\"total\":101}}", json!(vec![json!({"id": 1}); 100])), location: None },
            reply("GET /rest/api/2/search?jql=text%20~%20%22a%22&startAt=100&maxResults=1 HTTP/1.1", "200 OK", "{\"issues\":[{\"id\":2}],\"total\":101}"),
        ]);
        let result = client.search_issues("text ~ \"a\"", 101).unwrap();
        assert_eq!(result["issues"].as_array().unwrap().len(), 101);
        assert_eq!(client.version(), 2);
        server.join().unwrap();
    }

    #[test]
    fn search_resets_to_first_v2_page_after_later_token_fallback() {
        let (mut client, server) = fixture(vec![
            Reply {
                request: "GET /rest/api/3/search/jql?jql=ABC&maxResults=100 HTTP/1.1",
                status: "200 OK",
                content_type: "application/json",
                body: format!(
                    "{{\"issues\":{},\"nextPageToken\":\"more\"}}",
                    json!(vec![json!({"id": 3}); 100])
                ),
                location: None,
            },
            reply(
                "GET /rest/api/3/search/jql?jql=ABC&maxResults=1&nextPageToken=more HTTP/1.1",
                "404 Not Found",
                "{}",
            ),
            reply(
                "GET /rest/api/2/search?jql=ABC&startAt=0&maxResults=1 HTTP/1.1",
                "200 OK",
                "{\"issues\":[{\"id\":1}],\"total\":101}",
            ),
            Reply {
                request: "GET /rest/api/2/search?jql=ABC&startAt=0&maxResults=100 HTTP/1.1",
                status: "200 OK",
                content_type: "application/json",
                body: format!(
                    "{{\"issues\":{},\"total\":101}}",
                    json!(vec![json!({"id": 1}); 100])
                ),
                location: None,
            },
            reply(
                "GET /rest/api/2/search?jql=ABC&startAt=100&maxResults=1 HTTP/1.1",
                "200 OK",
                "{\"issues\":[{\"id\":2}],\"total\":101}",
            ),
        ]);
        let result = client.search_issues("ABC", 101).unwrap();
        assert_eq!(result["issues"].as_array().unwrap().len(), 101);
        assert_eq!(result["issues"][100]["id"], 2);
        assert_eq!(client.version(), 2);
        server.join().unwrap();
    }

    #[test]
    fn create_metadata_fallback_restarts_all_reads_in_v2() {
        let (mut client, server) = fixture(vec![
            reply("GET /rest/api/3/issue/createmeta/ABC/issuetypes?startAt=0&maxResults=100 HTTP/1.1", "200 OK", "{\"issueTypes\":[{\"id\":\"1\",\"name\":\"Task\"}],\"isLast\":true}"),
            reply("GET /rest/api/3/issue/createmeta/ABC/issuetypes/1?startAt=0&maxResults=100 HTTP/1.1", "410 Gone", "{}"),
            reply("GET /rest/api/2/issue/createmeta?projectKeys=ABC&expand=projects.issuetypes.fields HTTP/1.1", "200 OK", "{\"projects\":[{\"key\":\"ABC\",\"issuetypes\":[{\"id\":\"1\",\"name\":\"Task\",\"fields\":{\"summary\":{\"required\":true}}}]}]}"),
            reply("POST /rest/api/2/issue HTTP/1.1", "201 Created", "{\"key\":\"ABC-1\"}"),
        ]);
        let fields = client.create_fields("ABC", "Task").unwrap();
        assert_eq!(fields["fields"][0]["id"], "summary");
        assert_eq!(client.version(), 2);
        assert_eq!(
            client.create(&json!({"fields": {}})).unwrap()["key"],
            "ABC-1"
        );
        server.join().unwrap();
    }

    #[test]
    fn writes_are_single_shot_even_on_fallback_statuses() {
        for method in [
            reqwest::Method::POST,
            reqwest::Method::PUT,
            reqwest::Method::DELETE,
        ] {
            let (client, server) = fixture(vec![reply(
                match method {
                    reqwest::Method::POST => "POST /rest/api/3/issue HTTP/1.1",
                    reqwest::Method::PUT => "PUT /rest/api/3/issue HTTP/1.1",
                    _ => "DELETE /rest/api/3/issue HTTP/1.1",
                },
                "404 Not Found",
                "{}",
            )]);
            assert!(client.write(method, "/issue", None).is_err());
            assert_eq!(client.version(), 3);
            server.join().unwrap();
        }
    }

    #[test]
    fn internal_comment_posts_exact_plain_text_payload_with_existing_auth() {
        let (client, server) = fixture_with_cookie(
            vec![reply(
                "POST /rest/servicedeskapi/request/ABC%20%2F2/comment HTTP/1.1",
                "201 Created",
                r#"{"id":"10","public":false}"#,
            )],
            "session=test-cookie",
        );
        let result = client
            .add_internal_comment("ABC /2", "private\ntext")
            .unwrap();
        assert_eq!(result["public"], false);
        assert_eq!(client.version(), 3);
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            serde_json::from_str::<Value>(&requests[0].payload).unwrap(),
            json!({"body": "private\ntext", "public": false})
        );
        assert!(requests[0].basic_auth);
        assert!(requests[0].cookie);
        assert!(requests[0].json_content_type);
    }

    #[test]
    fn internal_comment_uses_fixed_gateway_root_when_cloud_id_is_configured() {
        let (mut client, server) = fixture(vec![]);
        client.config.cloud_id = "cloud-id".into();
        assert_eq!(
            client.service_desk_comment_url("ABC/1?public=true"),
            "https://api.atlassian.com/ex/jira/cloud-id/rest/servicedeskapi/request/ABC%2F1%3Fpublic%3Dtrue/comment"
        );
        assert!(server.join().unwrap().is_empty());
    }

    #[test]
    fn internal_comment_requires_explicit_private_response_and_never_replays() {
        for (status, content_type, body) in [
            ("201 Created", "application/json", r#"{"public":true}"#),
            ("201 Created", "application/json", r#"{"id":"10"}"#),
            ("201 Created", "application/json", r#"{"public":"false"}"#),
            ("204 No Content", "application/json", ""),
            ("200 OK", "text/html", "<html>login</html>"),
            ("200 OK", "application/json", "not json"),
            ("404 Not Found", "application/json", "{}"),
            ("403 Forbidden", "application/json", "{}"),
            ("400 Bad Request", "application/json", "{}"),
            ("302 Found", "text/html", "login"),
        ] {
            let (client, server) = fixture(vec![Reply {
                request: "POST /rest/servicedeskapi/request/ABC-1/comment HTTP/1.1",
                status,
                content_type,
                body: body.into(),
                location: None,
            }]);
            let error = client.add_internal_comment("ABC-1", "private").unwrap_err();
            assert!(!error.to_string().contains("secret"));
            assert_eq!(client.version(), 3);
            let requests = server.join().unwrap();
            assert_eq!(requests.len(), 1);
            assert_eq!(
                serde_json::from_str::<Value>(&requests[0].payload).unwrap(),
                json!({"body": "private", "public": false})
            );
        }
    }

    #[test]
    fn no_content_and_redirects_are_not_replayed() {
        let (client, server) = fixture(vec![
            Reply {
                request: "DELETE /rest/api/3/issue HTTP/1.1",
                status: "204 No Content",
                content_type: "",
                body: "".into(),
                location: None,
            },
            Reply {
                request: "GET /rest/api/3/field HTTP/1.1",
                status: "302 Found",
                content_type: "text/html",
                body: "login".into(),
                location: None,
            },
        ]);
        assert_eq!(
            client
                .write(reqwest::Method::DELETE, "/issue", None)
                .unwrap(),
            Value::Null
        );
        assert!(client
            .get("/field")
            .unwrap_err()
            .to_string()
            .contains("302"));
        assert_eq!(client.version(), 3);
        server.join().unwrap();
    }

    #[test]
    fn failed_comment_write_surfaces_jira_field_validation_details() {
        let (client, server) = fixture(vec![reply(
            "POST /rest/api/3/issue/ABC-1/comment HTTP/1.1",
            "400 Bad Request",
            r#"{"errorMessages":["Comment body is invalid"],"errors":{"body":"unsupported node"}}"#,
        )]);
        let payload = json!({"body": markdown_to_adf("**Heading**\n\n1. Item with `code`")});

        let error = client
            .write(
                reqwest::Method::POST,
                "/issue/ABC-1/comment",
                Some(&payload),
            )
            .unwrap_err();

        assert!(error.to_string().contains("Comment body is invalid"));
        assert!(error.to_string().contains("unsupported node"));
        server.join().unwrap();
    }
    #[test]
    fn encodes_path_segments() {
        assert_eq!(encoded("a/b ?"), "a%2Fb%20%3F");
    }
    #[test]
    fn adf_preserves_paragraphs() {
        let document = markdown_to_adf("one\n\ntwo");
        assert_eq!(document["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn markdown_to_adf_preserves_common_formatting() {
        let document =
            markdown_to_adf("# Title\n\n**bold** and *italic* with `code`\n\n- first\n- second");
        let content = document["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "heading");
        assert_eq!(content[0]["attrs"]["level"], 1);
        assert_eq!(content[1]["content"][0]["marks"][0]["type"], "strong");
        assert_eq!(content[1]["content"][2]["marks"][0]["type"], "em");
        assert_eq!(content[1]["content"][4]["marks"][0]["type"], "code");
        assert_eq!(content[2]["type"], "bulletList");
        assert_eq!(content[2]["content"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn markdown_to_adf_handles_reported_multiline_comment_body() {
        let comment = "Implementation plan\n\nBased on the Mercury till implementation (ModalDecline09, basket.attended.age-restriction.tsx, ProductTileNotificationLabel), tobacco age verification is a subtype of the existing AGE_RESTRICTION restriction (`subType: 'TOBACCO'`), with a backend-supplied `dateOfBirthCutoff` date rather than a client-calculated value. To match this in RAP, we plan to:\n\n1. **Schema** — add `subType: 'TOBACCO'` (optional) and `dateOfBirthCutoff` (ISO date, optional) to the age-restriction schemas (customerOrderBasket/customerOrderOrder) and the Intervention type.\n2. **Mapping** — pass subType/dateOfBirthCutoff through from the basket/order restriction data into the RAP intervention model.\n3. **Approval modal (InterventionPanel)** — when subType === 'TOBACCO':\n   - Header: \"Age verification required\"\n   - Message: \"Decline '09. No ID no sale.\"\n   - Underlying text: \"Date of birth must be before {dateOfBirthCutoff}\" (formatted dd/MM/yyyy, e.g. 01/01/2009), using the backend-supplied cutoff date — not the existing calculateValidDob client calculation.\n   - CTA remains Approve only (no Reject, consistent with current RAP behaviour).\n   - Non-tobacco age restrictions are unaffected and keep the existing \"Think 25\" / {age}+ age verification required flow.\n4. **Preceding-screen banner (InterventionColumn)** — when subType === 'TOBACCO', show \"Verify: Decline '09\" instead of \"Verify age: {age}+.\"\n5. **Tests/stories** — add a tobacco intervention factory variant, a new Storybook story, and test coverage for both the modal copy and the banner text.\n\nWill update this ticket once the change is implemented and ready for review.";
        let adf = markdown_to_adf(comment);
        let blocks = adf["content"].as_array().unwrap();

        assert_eq!(adf["type"], "doc");
        assert_eq!(blocks[0]["type"], "paragraph");
        assert_eq!(blocks[2]["type"], "orderedList");
        assert_eq!(blocks[2]["content"].as_array().unwrap().len(), 5);
        assert_eq!(blocks[2]["content"][0]["type"], "listItem");
        let ordered = blocks
            .iter()
            .find(|block| block["type"] == "orderedList")
            .expect("ordered plan list should be represented as an ADF orderedList");
        assert_eq!(ordered["content"].as_array().unwrap().len(), 5);
        assert!(has_adf_node_type(&ordered["content"][2], "bulletList"));
        assert!(has_adf_mark(&adf, "code"));

        let serialized = adf.to_string();
        for expected in [
            "subType: 'TOBACCO'",
            "dateOfBirthCutoff",
            "Decline '09. No ID no sale.",
            "Age verification required",
            "Verify: Decline '09",
        ] {
            assert!(serialized.contains(expected), "ADF lost {expected:?}");
        }
    }

    #[test]
    fn markdown_to_adf_converts_tables_to_valid_cell_nodes() {
        let document = markdown_to_adf("| Name | Value |\n| --- | --- |\n| a | b |");
        assert_eq!(document["content"][0]["type"], "table");
        assert_eq!(document["content"][0]["content"][0]["type"], "tableRow");
        assert_eq!(
            document["content"][0]["content"][0]["content"][0]["type"],
            "tableHeader"
        );
        assert_eq!(
            document["content"][0]["content"][1]["content"][0]["type"],
            "tableCell"
        );
    }

    #[test]
    fn markdown_to_plain_text_strips_formatting_syntax_and_keeps_link_target() {
        assert_eq!(
            markdown_to_plain_text("# **Title**\n\nSee [the docs](https://example.com)."),
            "Title\n\nSee the docs (https://example.com)."
        );
    }

    #[test]
    fn get_attachments_reads_the_attachment_field() {
        let (client, server) = fixture(vec![reply(
            "GET /rest/api/3/issue/ABC-1?fields=attachment HTTP/1.1",
            "200 OK",
            r#"{"fields":{"attachment":[{"id":"10050","filename":"screenshot.png","mimeType":"image/png","size":1024,"content":"http://example.com/content/10050"}]}}"#,
        )]);
        let attachments = client.get_attachments("ABC-1").unwrap();
        assert_eq!(attachments[0]["filename"], "screenshot.png");
        server.join().unwrap();
    }

    #[test]
    fn get_attachments_returns_empty_array_when_issue_has_none() {
        let (client, server) = fixture(vec![reply(
            "GET /rest/api/3/issue/ABC-1?fields=attachment HTTP/1.1",
            "200 OK",
            r#"{"fields":{}}"#,
        )]);
        assert_eq!(client.get_attachments("ABC-1").unwrap(), json!([]));
        server.join().unwrap();
    }

    #[test]
    fn download_attachment_writes_bytes_from_its_content_url() {
        let (client, server) = fixture(vec![Reply {
            request: "GET /attachment/content/10050 HTTP/1.1",
            status: "200 OK",
            content_type: "image/png",
            body: "fake-image-bytes".into(),
            location: None,
        }]);
        let content_url = format!("{}/attachment/content/10050", client.root());
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("screenshot.png");

        client.download_attachment(&content_url, &dest).unwrap();

        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "fake-image-bytes");
        server.join().unwrap();
    }

    #[test]
    fn download_attachment_follows_303_without_forwarding_credentials() {
        let (client, server) = fixture_with_cookie(
            vec![
                Reply {
                    request: "GET /attachment/content/10050 HTTP/1.1",
                    status: "303 See Other",
                    content_type: "text/plain",
                    body: String::new(),
                    location: Some("/signed/10050?token=temporary".into()),
                },
                Reply {
                    request: "GET /signed/10050?token=temporary HTTP/1.1",
                    status: "200 OK",
                    content_type: "image/png",
                    body: "fake-image-bytes".into(),
                    location: None,
                },
            ],
            "session=test-cookie",
        );
        let content_url = format!("{}/attachment/content/10050", client.root());
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("screenshot.png");

        client.download_attachment(&content_url, &dest).unwrap();

        assert_eq!(fs::read_to_string(&dest).unwrap(), "fake-image-bytes");
        let requests = server.join().unwrap();
        assert!(requests[0].basic_auth);
        assert!(requests[0].cookie);
        assert!(
            !requests[1].basic_auth,
            "redirect must not receive Basic credentials"
        );
        assert!(
            !requests[1].cookie,
            "redirect must not receive Jira cookies"
        );
    }

    #[test]
    fn download_attachment_refuses_a_content_url_on_a_different_host() {
        let (client, _server) = fixture(vec![]);
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("screenshot.png");

        let error = client
            .download_attachment("https://evil.example.com/steal", &dest)
            .unwrap_err();

        assert!(error.to_string().contains("Refusing to download"));
        assert!(!dest.exists());
    }

    #[test]
    fn download_attachment_refuses_a_non_http_scheme() {
        let (client, _server) = fixture(vec![]);
        let directory = tempfile::tempdir().unwrap();
        let dest = directory.path().join("screenshot.png");

        let error = client
            .download_attachment("file:///etc/passwd", &dest)
            .unwrap_err();

        assert!(error.to_string().contains("non-HTTP"));
        assert!(!dest.exists());
    }
}
