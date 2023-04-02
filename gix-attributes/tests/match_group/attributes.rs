#[test]
fn baseline() -> crate::Result {
    baseline::validate("basics")
}

mod baseline {
    use bstr::{BStr, ByteSlice};
    use gix_attributes::StateRef;

    pub fn validate(name: &str) -> crate::Result {
        let dir = gix_testtools::scripted_fixture_read_only("make_attributes_baseline.sh")?;
        let repo_dir = dir.join(name);
        let input = std::fs::read(repo_dir.join("baseline"))?;
        // TODO: everything with ignorecase (tolower, expect same results)

        for (rela_path, attributes) in (Expectations { lines: input.lines() }) {
            dbg!(rela_path, attributes);
        }

        Ok(())
    }

    pub struct Expectations<'a> {
        pub lines: bstr::Lines<'a>,
    }

    impl<'a> Iterator for Expectations<'a> {
        type Item = (
            &'a BStr,
            // Names might refer to attributes or macros
            Vec<(gix_attributes::NameRef<'a>, gix_attributes::StateRef<'a>)>,
        );

        fn next(&mut self) -> Option<Self::Item> {
            let path = self.lines.next()?;
            let mut assignments = Vec::new();
            loop {
                let line = self.lines.next()?;
                if line.is_empty() {
                    return Some((path.as_bstr(), assignments));
                }

                let mut prev = None;
                let mut tokens = line.splitn(3, |b| {
                    let is_match = *b == b' ' && prev.take() == Some(b':');
                    prev = Some(*b);
                    is_match
                });

                if let Some(((_path, attr), info)) = tokens.next().zip(tokens.next()).zip(tokens.next()) {
                    let state = match info {
                        b"set" => StateRef::Set,
                        b"unset" => StateRef::Unset,
                        b"unspecified" => StateRef::Unspecified,
                        _ => StateRef::Value(info.as_bstr()),
                    };
                    let attr = attr.trim_end_with(|b| b == ':');
                    assignments.push((
                        gix_attributes::NameRef::try_from(attr.as_bstr()).expect("valid attributes"),
                        state,
                    ));
                } else {
                    unreachable!("invalid line format: {line:?}", line = line.as_bstr())
                }
            }
        }
    }
}
