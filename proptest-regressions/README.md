# Property-test regressions

When a property test finds a failing case, proptest saves the seed of the
smallest failing case here. On later runs it replays these seeds before it
generates new cases, so a failure that was found once is found again.

The files are committed on purpose. Each one mirrors the source file whose
test wrote it: a test in `src/typeset/images/size/props.rs` saves to
`typeset/images/size/props.txt`.

More in the proptest book on
[failure persistence](https://proptest-rs.github.io/proptest/proptest/failure-persistence.html)
and in the docs of
[`FileFailurePersistence`](https://docs.rs/proptest/latest/proptest/test_runner/enum.FileFailurePersistence.html),
which decides where the files go.
