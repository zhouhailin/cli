use std::cmp::Ordering;

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
    let s = s.trim_start_matches(|c| c == 'v' || c == 'V');
    s.parse().ok()
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
}
