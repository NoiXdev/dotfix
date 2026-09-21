use crate::drift::Counts;

/// One line for the shell hook, or `None` when everything is in sync.
///
/// Silence matters: a tool that greets every new terminal tab gets disabled
/// within a week.
pub fn render(counts: &Counts) -> Option<String> {
    let mut parts = Vec::new();

    if counts.incoming > 0 {
        parts.push(plural(counts.incoming, "change", "changes"));
    }
    if counts.removed > 0 {
        parts.push(format!("{} to remove", counts.removed));
    }
    if counts.local_edits > 0 {
        parts.push(plural(
            counts.local_edits,
            "changed config",
            "changed configs",
        ));
    }
    if counts.unmanaged > 0 {
        parts.push(format!("{} unmanaged", counts.unmanaged));
    }

    if parts.is_empty() {
        return None;
    }

    // Only unmanaged drift means there is nothing to apply — point at adopt.
    let action = if counts.incoming == 0 && counts.removed == 0 && counts.local_edits == 0 {
        "dotfix adopt"
    } else {
        "dotfix apply"
    };

    Some(format!("↯ dotfix: {}   →  {action}", parts.join(" · ")))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drift::Counts;

    #[test]
    fn nothing_to_report_yields_no_line() {
        assert_eq!(render(&Counts::default()), None);
    }

    #[test]
    fn singular_and_plural_are_both_readable() {
        let one = render(&Counts {
            incoming: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(one, "↯ dotfix: 1 change   →  dotfix apply");

        let many = render(&Counts {
            incoming: 2,
            local_edits: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            many,
            "↯ dotfix: 2 changes · 1 changed config   →  dotfix apply"
        );
    }

    #[test]
    fn unmanaged_only_points_at_adopt_instead_of_apply() {
        let line = render(&Counts {
            unmanaged: 3,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(line, "↯ dotfix: 3 unmanaged   →  dotfix adopt");
    }

    #[test]
    fn the_line_is_a_single_line() {
        let line = render(&Counts {
            incoming: 1,
            unmanaged: 1,
            removed: 1,
            local_edits: 1,
        })
        .unwrap();
        assert!(!line.contains('\n'));
    }
}
