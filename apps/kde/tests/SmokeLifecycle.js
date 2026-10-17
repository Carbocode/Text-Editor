// Regression drivers choose discard explicitly; production windows now protect quit.
function quit(controller, lifecycle, code) {
    if (code === 0 && !controller.prepare_to_quit(false)) {
        console.error("Smoke quit rejected: " + controller.error_message)
        code = 1
    }
    lifecycle.finishQuit(code)
}
