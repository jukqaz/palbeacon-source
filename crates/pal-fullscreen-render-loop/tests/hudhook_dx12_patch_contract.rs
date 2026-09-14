use std::{fs, path::PathBuf};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("render-loop crate should live directly below the workspace crates directory")
        .to_path_buf()
}

#[test]
fn workspace_uses_the_dx12_texture_state_patch() {
    let workspace = workspace_root();
    let workspace_manifest =
        fs::read_to_string(workspace.join("Cargo.toml")).expect("read workspace Cargo.toml");
    assert!(
        workspace_manifest.contains("hudhook = { path = \"patches/hudhook-0.9.2\" }"),
        "the workspace must keep using the reviewed local hudhook patch"
    );

    let source =
        fs::read_to_string(workspace.join("patches/hudhook-0.9.2/src/renderer/backend/dx12.rs"))
            .expect("read patched hudhook DX12 backend");
    assert!(
        source.contains("create_heaps(&device, &command_queue, node_mask)"),
        "the texture heap must receive the renderer's ordered direct queue"
    );
    assert!(
        !source.contains("device.CreateCommandQueue"),
        "texture replacement must not race rendering on an independent command queue"
    );
    let upload = source
        .split_once("unsafe fn upload_texture")
        .expect("find DX12 texture upload implementation")
        .1;

    let pre_copy_barrier = upload
        .find("if let Some(barrier) = before_copy_barrier.as_ref()")
        .expect("replace uploads must transition the texture to COPY_DEST");
    let copy = upload
        .find("CopyTextureRegion")
        .expect("texture upload must contain a copy command");
    let post_copy_barrier = upload
        .find("std::slice::from_ref(&after_copy_barrier)")
        .expect("uploads must restore PIXEL_SHADER_RESOURCE after the copy");
    let submit = upload
        .find("ExecuteCommandLists")
        .expect("texture upload must submit its command list");
    let tracked_state = upload
        .find("stable_state = barrier_plan.stable_state_after_upload()")
        .expect("the submitted final resource state must be tracked");

    assert!(
        pre_copy_barrier < copy,
        "the COPY_DEST barrier must precede the copy"
    );
    assert!(
        copy < post_copy_barrier,
        "the shader-resource barrier must follow the copy"
    );
    assert!(
        post_copy_barrier < submit,
        "both barriers must be recorded before submission"
    );
    assert!(
        submit < tracked_state,
        "state tracking must change only after submission"
    );
}

#[test]
fn workspace_uses_the_escape_preserving_keyboard_filter_patch() {
    let workspace = workspace_root();
    let filter =
        fs::read_to_string(workspace.join("patches/hudhook-0.9.2/src/renderer/msg_filter.rs"))
            .expect("read patched hudhook message filter");
    let pipeline =
        fs::read_to_string(workspace.join("patches/hudhook-0.9.2/src/renderer/pipeline.rs"))
            .expect("read patched hudhook pipeline");

    assert!(filter.contains("InputKeyboardExceptEscape"));
    assert!(filter.contains("wparam.0 != usize::from(VK_ESCAPE.0)"));
    assert!(pipeline.contains("message_filter.is_blocking(msg, wparam)"));
}
