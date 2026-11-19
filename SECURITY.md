# Security policy

Please report vulnerabilities privately through GitHub: open the repository's
**Security** tab and choose **Report a vulnerability**. Do not open a public
issue for a security problem.

Supported versions: the latest commit on `main` until the first release; after
that, the latest release.

The guard hook and the history and surface checks are defence in depth, not a
sandbox. Run agents with bypassed permissions only inside an isolated container
or VM.
