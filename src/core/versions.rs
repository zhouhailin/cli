use std::cmp::Ordering;

use anyhow::{anyhow, Result};

/// 按点分段的数字版本比较（3.9.10 > 3.9.9；v22.12.0 > v22.9.0）
pub fn compare(a: &str, b: &str) -> Ordering {
    let pa: Vec<u64> = a.split('.').filter_map(parse_segment).collect();
    let pb: Vec<u64> = b.split('.').filter_map(parse_segment).collect();
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    Ordering::Equal
}

/// 解析单个版本段，容忍 v 前缀（v22 → 22）
fn parse_segment(s: &str) -> Option<u64> {
    let s = s.trim_start_matches(['v', 'V']);
    s.parse().ok()
}

/// 去除版本号前缀 v/V（v0.1.1 -> 0.1.1），无前缀原样返回
pub fn parse_tag(tag: &str) -> &str {
    tag.strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
}

/// 从目录页 HTML 提取纯数字点分版本目录名（3.9.9、1.0.6），降序；过滤 rc/beta/milestone 等非纯数字段
pub fn parse_version_dirs(html: &str) -> Result<Vec<String>> {
    let mut versions: Vec<String> = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(r#"<a href=""#) {
        let after = &rest[start + 9..];
        let Some(end) = after.find(r#"">"#) else {
            break;
        };
        let href = &after[..end];
        if let Some(dir) = href.strip_suffix('/') {
            // 纯数字点分目录；要求至少含一个数字，过滤 ../ 等纯点目录
            if dir.chars().all(|c| c.is_ascii_digit() || c == '.')
                && dir.chars().any(|c| c.is_ascii_digit())
            {
                versions.push(dir.to_string());
            }
        }
        rest = &after[end..];
    }
    if versions.is_empty() {
        return Err(anyhow!("未解析到任何版本"));
    }
    versions.sort_by(|a, b| compare(b, a));
    Ok(versions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_orders_numeric_segments() {
        assert_eq!(compare("3.9.10", "3.9.9"), Ordering::Greater);
        assert_eq!(compare("3.9.9", "3.9.9"), Ordering::Equal);
        assert_eq!(compare("3.8", "3.9.1"), Ordering::Less);
    }

    #[test]
    fn compare_handles_v_prefix_and_padding() {
        assert_eq!(compare("v22.12.0", "v22.9.0"), Ordering::Greater);
        assert_eq!(compare("1.22.6", "1.21.13"), Ordering::Greater);
    }

    #[test]
    fn parse_tag_strips_v_prefix() {
        assert_eq!(parse_tag("v0.1.1"), "0.1.1");
        assert_eq!(parse_tag("V1.2.3"), "1.2.3");
        assert_eq!(parse_tag("0.1.1"), "0.1.1");
        assert_eq!(parse_tag(""), "");
    }

    #[test]
    fn parse_version_dirs_filters_non_numeric_and_sorts_desc() {
        let html = r#"<html><body>
          <a href="1.0.6/">1.0.6/</a>
          <a href="2.0.0-rc-3/">2.0.0-rc-3/</a>
          <a href="1.0-m6/">1.0-m6/</a>
          <a href="1.0.5/">1.0.5/</a>
          <a href="README.html">README</a>
        </body></html>"#;
        let list = parse_version_dirs(html).unwrap();
        assert_eq!(list, vec!["1.0.6", "1.0.5"]);
    }

    #[test]
    fn parse_version_dirs_rejects_empty() {
        assert!(parse_version_dirs("<html>no links</html>").is_err());
    }

    #[test]
    fn parse_version_dirs_ignores_dot_dirs() {
        // 镜像站列表页含 ../ 父目录链接（纯点目录），必须过滤
        let html = r#"<html><body>
          <a href="../">../</a>
          <a href="1.0.6/">1.0.6/</a>
        </body></html>"#;
        let list = parse_version_dirs(html).unwrap();
        assert_eq!(list, vec!["1.0.6"]);
    }
}
