# Rules for this folder

- Do not delete a seed because its test passes now. Remove one only when the
  property it belongs to no longer exists.
- Do not edit the `cc` lines by hand. Proptest writes them.
- Commit a new seed in the same commit as the fix for its failure.

See README.md for why the folder exists.
