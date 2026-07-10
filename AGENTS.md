<!-- SPECKIT START -->
For additional context about technologies to be used, project structure,
shell commands, and other important information, read the current plan:
specs/071-transport-crypto-offload/plan.md
<!-- SPECKIT END -->

GitHub pull requests should be made on the occamsshavingkit/async-opcua fork, never on the upstream source. Only open a PR on the upstream source on an explicit request from the user. 

**Pre-PR gate**: Before opening any pull request, run the local CI playbook via `tools/ci-playbook.sh --ci`. All steps must pass before the PR is created. If any step fails, fix the issue and re-run until green.

This is an asyncronous codebase. You MUST NOT implement any new locks, mutexes, semaphors or other blocking code UNLESS you have exhausted all other options. 
If any new blocking code is implemented, you MUST run the skill audit-locks on the new code to assess its impact and get mitigation suggestions. 

You MUST USE the tools/ci-playbook.sh to confirm that a pull request will pass CI before creating the pull request.
