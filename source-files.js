var N = null;var sourcesIndex = {};
sourcesIndex["task_maker"] = {"name":"","files":["main.rs"]};
sourcesIndex["task_maker_cache"] = {"name":"","files":["entry.rs","key.rs","lib.rs","storage.rs"]};
sourcesIndex["task_maker_dag"] = {"name":"","files":["dag.rs","execution.rs","execution_group.rs","file.rs","lib.rs"]};
sourcesIndex["task_maker_diagnostics"] = {"name":"","files":["lib.rs","span.rs"]};
sourcesIndex["task_maker_exec"] = {"name":"","dirs":[{"name":"executors","files":["local_executor.rs","mod.rs","remote_executor.rs"]}],"files":["check_dag.rs","client.rs","detect_exe.rs","executor.rs","find_tools.rs","lib.rs","proto.rs","sandbox.rs","sandbox_runner.rs","scheduler.rs","worker.rs","worker_manager.rs"]};
sourcesIndex["task_maker_format"] = {"name":"","dirs":[{"name":"ioi","dirs":[{"name":"dag","dirs":[{"name":"task_type","files":["batch.rs","communication.rs","mod.rs"]}],"files":["checker.rs","input_generator.rs","input_validator.rs","mod.rs","output_generator.rs"]},{"name":"format","dirs":[{"name":"italian_yaml","files":["cases_gen.rs","gen_gen.rs","mod.rs","static_inputs.rs"]}],"files":["mod.rs"]},{"name":"sanity_checks","files":["att.rs","mod.rs","sol.rs","statement.rs","subtasks.rs","task.rs"]},{"name":"statement","files":["asy.rs","booklet.rs","mod.rs","statement.rs"]}],"files":["curses_ui.rs","finish_ui.rs","mod.rs","task_info.rs","ui_state.rs"]},{"name":"terry","dirs":[{"name":"dag","files":["mod.rs"]},{"name":"format","files":["mod.rs"]},{"name":"sanity_checks","files":["checker.rs","mod.rs","statement.rs","task.rs"]}],"files":["curses_ui.rs","finish_ui.rs","mod.rs","task_info.rs","ui_state.rs"]},{"name":"ui","files":["curses.rs","json.rs","mod.rs","print.rs","raw.rs","silent.rs","ui_message.rs"]}],"files":["detect_format.rs","lib.rs","sanity_checks.rs","solution.rs","source_file.rs","tag.rs","task_format.rs"]};
sourcesIndex["task_maker_lang"] = {"name":"","dirs":[{"name":"languages","files":["c.rs","cpp.rs","csharp.rs","javascript.rs","mod.rs","pascal.rs","python.rs","rust.rs","shell.rs"]}],"files":["grader_map.rs","language.rs","lib.rs","source_file.rs"]};
sourcesIndex["task_maker_rust"] = {"name":"","dirs":[{"name":"tools","dirs":[{"name":"find_bad_case","files":["curses_ui.rs","dag.rs","finish_ui.rs","mod.rs","state.rs"]}],"files":["add_solution_checks.rs","booklet.rs","clear.rs","fuzz_checker.rs","gen_autocompletion.rs","mod.rs","opt.rs","reset.rs","sandbox.rs","server.rs","task_info.rs","typescriptify.rs","worker.rs"]}],"files":["context.rs","copy_dag.rs","error.rs","lib.rs","local.rs","opt.rs","remote.rs","sandbox.rs"]};
sourcesIndex["task_maker_store"] = {"name":"","files":["index.rs","lib.rs","read_file_iterator.rs"]};
sourcesIndex["task_maker_tools"] = {"name":"","files":["main.rs"]};
createSourceSidebar();
