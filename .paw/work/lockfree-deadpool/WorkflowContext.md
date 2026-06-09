# WorkflowContext

Work Title: Lockfree Deadpool
Work ID: lockfree-deadpool
Base Branch: main
Target Branch: users/andrewkhoma/lockfree
Execution Mode: current-checkout
Repository Identity: github.com/andrewkhoma/deadpool@a40e6f35114ecc43f4d956ab2599a470c2bb7573
Execution Binding: none
Workflow Mode: full
Review Strategy: local
Review Policy: milestones
Session Policy: continuous
Final Agent Review: enabled
Final Review Mode: society-of-thought
Final Review Interactive: smart
Final Review Models: gpt-5.5, gemini-3.1-pro-preview, claude-opus-4.8
Final Review Specialists: all
Final Review Interaction Mode: parallel
Final Review Specialist Models: none
Final Review Perspectives: auto
Final Review Perspective Cap: 2
Implementation Model: none
Plan Generation Mode: single-model
Plan Generation Models: gpt-5.5, gemini-3.1-pro-preview, claude-opus-4.8
Planning Docs Review: enabled
Planning Review Mode: society-of-thought
Planning Review Interactive: smart
Planning Review Models: gpt-5.5, gemini-3.1-pro-preview, claude-opus-4.8
Planning Review Specialists: all
Planning Review Interaction Mode: parallel
Planning Review Specialist Models: none
Planning Review Perspectives: auto
Planning Review Perspective Cap: 2
Custom Workflow Instructions: none
Initial Prompt: Update the Deadpool library in the current project and current branch to make it lockfree for the application. Use the drafted analogue of the Deadpool library for PostgreSQL under .paw/documentdb_core_local_pool as a reference. The application gateway is under .paw/documentdb/pg_documentdb_gw and runs on top of PostgreSQL. For thread-per-core ADO PR https://msdata.visualstudio.com/CosmosDB/_git/pgmongo/pullrequest/2031705, ask whether it should be pulled under .paw for reference if needed.
Issue URL: none
Remote: origin
Artifact Lifecycle: commit-and-clean
Artifact Paths: auto-derived
Additional Inputs: reference pool .paw/documentdb_core_local_pool; gateway .paw/documentdb/pg_documentdb_gw; external ADO PR https://msdata.visualstudio.com/CosmosDB/_git/pgmongo/pullrequest/2031705
