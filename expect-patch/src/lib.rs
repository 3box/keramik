use expect_test::{Expect, ExpectFile};

/// Defines a common interface for asserting an expectation.
///
/// The types expect_test::Expect, expect_test::ExpectFile and ExpectPatch all implement this trait.
pub trait Expectation {
    /// Make an assertion about the actual value compared to the expectation.
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug);
}

/// Defines a common interface exposing the expected data for an Expectation.
///
/// This allows ExpectPatch to be composed of either Expect or ExpectFile.
trait ExpectData {
    /// The data that is expected.
    fn data(&self) -> String;
}

/// Wraps an expectation with the ability to patch that expectation.
///
/// ```
/// use expect_test::{expect, expect_file, ExpectFile};
/// use expect_patch::ExpectPatch;
///
/// let mut expect: ExpectPatch<ExpectFile> = expect_file!["path/to/large/test/fixture"].into();
/// expect.patch(expect![[r#"
/// --- original
/// +++ modified
/// @@ -1,2 +1,3 @@
///  Small patch
///  to the larger
/// +expectation data
/// "#]]);
/// ```
///
/// If you find yourself manually writing the patch text,
/// instead use `UPDATE_EXPECT=1 cargo test` to update the patch text itself.
#[derive(Debug)]
pub struct ExpectPatch<E> {
    original: E,
    patch: Option<Expect>,
}

impl<E> ExpectPatch<E> {
    pub fn patch(&mut self, patch: Expect) {
        self.patch = Some(patch)
    }
}

impl From<ExpectFile> for ExpectPatch<ExpectFile> {
    fn from(value: ExpectFile) -> Self {
        Self {
            original: value,
            patch: None,
        }
    }
}

impl From<Expect> for ExpectPatch<Expect> {
    fn from(value: Expect) -> Self {
        Self {
            original: value,
            patch: None,
        }
    }
}

impl<E> Expectation for ExpectPatch<E>
where
    E: ExpectData + Expectation,
{
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        if let Some(patch) = &self.patch {
            let actual = &format!("{:#?}", actual);
            let original = self.original.data();
            // Ensure both end in a new line to avoid noisy patch data
            let original = original.trim().to_owned() + "\n";
            let actual = actual.trim().to_owned() + "\n";
            let actual_patch = diffy::create_patch(&original, &actual);
            patch.assert_eq(&format!("{actual_patch}"));
        } else {
            self.original.assert_debug_eq(actual)
        }
    }
}

impl Expectation for Expect {
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        self.assert_debug_eq(actual)
    }
}
impl ExpectData for Expect {
    fn data(&self) -> String {
        self.data().to_owned()
    }
}

impl Expectation for ExpectFile {
    fn assert_debug_eq(&self, actual: &impl std::fmt::Debug) {
        self.assert_debug_eq(actual)
    }
}
impl ExpectData for ExpectFile {
    fn data(&self) -> String {
        // This is taken from the internals of expect_file, maybe we can submit a change to expose
        // the file contents directly?
        let path_of_test_file = std::path::Path::new(self.position).parent().unwrap();
        static WORKSPACE_ROOT: once_cell::sync::OnceCell<std::path::PathBuf> =
            once_cell::sync::OnceCell::new();
        let abs_path = WORKSPACE_ROOT
            .get_or_try_init(|| {
                // Until https://github.com/rust-lang/cargo/issues/3946 is resolved, this
                // is set with a hack like https://github.com/rust-lang/cargo/issues/3946#issuecomment-973132993
                if let Ok(workspace_root) = std::env::var("CARGO_WORKSPACE_DIR") {
                    return Ok(workspace_root.into());
                }

                // If a hack isn't used, we use a heuristic to find the "top-level" workspace.
                // This fails in some cases, see https://github.com/rust-analyzer/expect-test/issues/33
                let my_manifest = std::env::var("CARGO_MANIFEST_DIR")?;
                let workspace_root = std::path::Path::new(&my_manifest)
                    .ancestors()
                    .filter(|it| it.join("Cargo.toml").exists())
                    .last()
                    .unwrap()
                    .to_path_buf();

                Ok(workspace_root)
            })
            .unwrap_or_else(|_: std::env::VarError| {
                panic!(
                    "No CARGO_MANIFEST_DIR env var and the path is relative: {}",
                    path_of_test_file.display()
                )
            })
            .join(path_of_test_file)
            .join(&self.path);

        std::fs::read_to_string(abs_path)
            .unwrap_or_default()
            .replace("\r\n", "\n")
    }
}
