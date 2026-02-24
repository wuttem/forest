# Agent Instructions

This file contains the core tenets and guidelines for AI Agents working on the Forest IoT Platform. Please refer to these principles when writing code, refactoring, or designing new features.

## General Tenets & Priorities

1. **Simplicity First**
   We always prefer simple solutions. Avoid over-engineering, unnecessary abstractions, or overly clever code. 

2. **Performance**
   High performance is critical, especially in the context of the MQTT broker and message handling. Ensure that new changes do not introduce latency or unnecessary overhead.

3. **Minimal Code**
   Write only as much code as is strictly needed to solve the problem. Less code means fewer bugs and easier maintenance.

4. **Structured and Commented**
   Everything should be nicely structured. If a file or function gets too big, break it down. Add comments where needed, focusing on *why* complex decisions were made rather than just what the code does.

5. **Clear Documentation**
   Provide clear explanations in the documentation for any new features or architectural changes.

6. **Pragmatic Testing**
   Include a good amount of tests to verify functionality, but do not write *too many* tests. Focus on testing critical paths and business logic rather than obsessively testing every private helper function or implementation detail.

## Workflow Guidelines

1. **Branching Strategy**
   For every new conversation or workstream, always use a new git branch. You may commit and push to this branch as needed during your work.

2. **Always Clean Up**
   Ensure any temporary files, helper scripts (like Python split scripts), or debug logs created during a task are deleted before finalizing the workstream.

3. **Clean Commits**
   Everything you commit must be clean, correctly formatted, and properly functioning. Always include necessary tests and documentation updates alongside your code changes.

4. **Finishing a Workstream**
   Once you've completed a workstream, perform a final cleanliness check. **Always secure confirmation from the user** before merging your completed workstream branch back into `main`.
