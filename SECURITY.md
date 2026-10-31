# Security Policy

CFDCraft reads study files, writes simulation case folders and can run a solver in a container on a remote host. Treat study files, paths, case directories and command parameters as potentially malicious.

## Reporting a vulnerability

Please give the maintainers a reasonable chance to investigate and fix a problem before you publish exploitable details or proof-of-concept code.

Send security reports to security@milleleaf.com. Send a short, non-exploitable summary first and we will agree a private way to share details. Do not post a working exploit, a secret or a sensitive crash dump in a public issue.

Questions and contributions: hello@milleleaf.com.

## What to include

- the CFDCraft version or commit, and the operating system and architecture;
- the affected component or file;
- the security impact and what access an attacker needs;
- exact steps to reproduce, with the expected and the actual behaviour;
- logs or backtraces with credentials, personal paths and unrelated system details removed.

## Scope

Reports may cover, for example:

- study or case-file parsing that crashes, hangs or exhausts memory;
- writing a case that contains code-executing entries, or accepting such entries from input;
- path traversal when writing a case or reading results;
- unsafe handling of credentials, host addresses or remote commands;
- dependency or build-supply-chain problems.

Reports about the behaviour of OpenFOAM itself should go to the OpenFOAM project.

## Test inputs

Do not attach a harmful or non-redistributable file to a public issue. Describe its format, size and hash, the effect you see and the command that reproduces it, then agree on a private transfer.
