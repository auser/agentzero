Can you archive this sprint in `specs/sprints/<number>-<name>.md` and then create a new SPRINT for us to work on? I want you to pull in all those items you listed

---

I want to focus on shrinking the runtime as much as possible.

---

Why no WASI?
Can we support cross-platform for plugins?
I think we want plugins that are in-development to be loaded from the current working directory, not just the $DATA_DIR/plugins and $PROJECT/.agentzero/plugins
What are all the feature flags and can we have them listed in the main crate so we can build the main crate with them?
What parts of the system as it is today should be converted to a plugin?
Can you help create a plugin example?
We need the ease-of-use, performance, simplicity, core agent-platform, library crate publishing with FFI as core features as well as minimal memory use, and safety and security are highly important features.
We want a registry. 
How would plugins have the permission to use host tools/callbacks?
I do want hot-reload for plugins that are IN DEVELOPMENT (cwd)
Can we have a macro to help us create plugins?
Can we create plugins using the FFI?