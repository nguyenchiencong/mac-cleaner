# Direct Process Execution and Explicit Irreversible Semantics

Delegating cleanup to external tools (like `xcrun simctl` or `brew cleanup`) via shell strings introduces quoting risks, and such tools bypass native Trash recoverability. We decided to execute external commands exclusively through structured argument arrays with timeout and output limits, and to classify their resulting actions as explicitly non-restorable in plans and manifests. This eliminates injection vulnerabilities and ensures users are clearly warned before irreversible actions run.
