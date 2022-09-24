var sourcesIndex = JSON.parse('{\
"task_maker":["",[],["main.rs"]],\
"task_maker_cache":["",[],["entry.rs","key.rs","lib.rs","storage.rs"]],\
"task_maker_dag":["",[],["dag.rs","execution.rs","execution_group.rs","file.rs","lib.rs"]],\
"task_maker_diagnostics":["",[],["lib.rs","span.rs"]],\
"task_maker_exec":["",[["executors",[],["local_executor.rs","mod.rs","remote_executor.rs"]]],["check_dag.rs","client.rs","detect_exe.rs","executor.rs","find_tools.rs","lib.rs","proto.rs","sandbox.rs","sandbox_runner.rs","scheduler.rs","worker.rs","worker_manager.rs"]],\
"task_maker_format":["",[["ioi",[["dag",[["task_type",[],["batch.rs","communication.rs","mod.rs"]]],["checker.rs","input_generator.rs","input_validator.rs","mod.rs","output_generator.rs"]],["format",[["italian_yaml",[],["cases_gen.rs","gen_gen.rs","mod.rs","static_inputs.rs"]]],["mod.rs"]],["sanity_checks",[],["att.rs","mod.rs","sol.rs","statement.rs","subtasks.rs","task.rs"]],["statement",[],["asy.rs","booklet.rs","mod.rs","statement.rs"]]],["curses_ui.rs","finish_ui.rs","mod.rs","task_info.rs","ui_state.rs"]],["terry",[["dag",[],["mod.rs"]],["format",[],["mod.rs"]],["sanity_checks",[],["checker.rs","mod.rs","statement.rs","task.rs"]]],["curses_ui.rs","finish_ui.rs","mod.rs","task_info.rs","ui_state.rs"]],["ui",[],["curses.rs","json.rs","mod.rs","print.rs","raw.rs","silent.rs","ui_message.rs"]]],["detect_format.rs","lib.rs","sanity_checks.rs","solution.rs","source_file.rs","tag.rs","task_format.rs","testcase_score_status.rs"]],\
"task_maker_lang":["",[["languages",[],["c.rs","cpp.rs","csharp.rs","javascript.rs","mod.rs","pascal.rs","python.rs","rust.rs","shell.rs"]]],["grader_map.rs","language.rs","lib.rs","source_file.rs"]],\
"task_maker_rust":["",[["tools",[["find_bad_case",[],["curses_ui.rs","dag.rs","finish_ui.rs","mod.rs","state.rs"]]],["add_solution_checks.rs","booklet.rs","clear.rs","fuzz_checker.rs","gen_autocompletion.rs","mod.rs","opt.rs","reset.rs","sandbox.rs","server.rs","task_info.rs","typescriptify.rs","worker.rs"]]],["context.rs","copy_dag.rs","error.rs","lib.rs","local.rs","opt.rs","remote.rs","sandbox.rs"]],\
"task_maker_store":["",[],["index.rs","lib.rs","read_file_iterator.rs"]],\
"task_maker_tools":["",[],["main.rs"]]\
}');
createSourceSidebar();
