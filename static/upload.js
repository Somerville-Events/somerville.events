const body = document.body;
const cameraUi = document.querySelector(".camera-ui");
const video = document.querySelector("video");
const shutterBtn = document.querySelector(".camera-ui button.primary");
const uploadBtn = document.querySelector(".camera-ui button:not(.primary)");
const canvas = document.querySelector("canvas");
const skeleton = document.querySelector(".skeleton");

const form = document.querySelector("form");
const fileInput = document.querySelector('input[type="file"]');
const imagePreview = document.querySelector("form img");
const statusPanel = document.querySelector(".status-panel");
const progressBar = statusPanel?.querySelector("progress");
const progressText = statusPanel?.querySelector("output");

let stream = null;

if (
    !cameraUi ||
    !video ||
    !shutterBtn ||
    !uploadBtn ||
    !canvas ||
    !skeleton ||
    !form ||
    !fileInput ||
    !imagePreview ||
    !statusPanel ||
    !progressBar ||
    !progressText
) {
    throw new Error("One or more elements not found");
}

const syncInput = document.createElement("input");
syncInput.type = "hidden";
syncInput.name = "sync";
syncInput.value = "true";
form.appendChild(syncInput);

function setButtonsDisabled(disabled) {
    shutterBtn.disabled = disabled;
    if (uploadBtn) uploadBtn.disabled = disabled;
    const submitBtn = form.querySelector('button[type="submit"]');
    if (submitBtn) submitBtn.disabled = disabled;
}

function updateProgress(percent) {
    progressBar.value = percent;
    progressText.textContent = `Uploading ${percent}%`;
}

function showUploadProgress(percent) {
    statusPanel.classList.remove("hidden");
    progressBar.removeAttribute("value"); // Ensure it's not indeterminate initially if we set a value
    progressBar.max = 100;
    updateProgress(percent);

    // Hide submit button
    const submitBtn = form.querySelector('button[type="submit"]');
    if (submitBtn) submitBtn.style.display = 'none';
}

function showProcessing() {
    // Switch to indeterminate state
    progressBar.removeAttribute("value");
    progressText.textContent = "Processing in background. You can close this app.";
}

function submitFormWithFile(file) {
    const selectedFile = file || fileInput.files?.[0];
    if (!selectedFile) return;

    const formData = new FormData(form);
    formData.set("image", selectedFile, selectedFile.name);

    setButtonsDisabled(true);
    showUploadProgress(0);

    const xhr = new XMLHttpRequest();
    xhr.open("POST", form.action);
    xhr.responseType = "document";

    xhr.upload.onprogress = (event) => {
        if (event.lengthComputable) {
            const percent = Math.round((event.loaded / event.total) * 100);
            updateProgress(percent);
        }
    };

    xhr.upload.onload = () => {
        updateProgress(100);
        showProcessing();
    };

    xhr.onload = () => {
        const targetUrl =
            xhr.responseURL || xhr.getResponseHeader("Location") || "/upload-success";
        window.location.href = targetUrl;
    };

    xhr.onerror = () => {
        window.location.href =
            "/upload-error?message=" + encodeURIComponent("Network error while uploading.");
    };

    xhr.send(formData);
}

// Initialize Camera
if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
    try {
        stream = await navigator.mediaDevices.getUserMedia({
            video: { facingMode: "environment" },
        });
        video.srcObject = stream;

        // Wait for video to be ready before showing
        video.onloadedmetadata = () => {
            skeleton.classList.add("hidden");
            video.classList.remove("loading");
        };

        // Upgrade to Camera Mode
        body.classList.remove("no-camera");

        // Handle Shutter - Immediate Upload
        shutterBtn.addEventListener("click", () => {
            if (!stream) return;

            // Visual feedback
            shutterBtn.innerHTML = '<span class="spinner"></span> Uploading...';
            setButtonsDisabled(true);

            // Freeze video to show what was captured
            video.pause();

            canvas.width = video.videoWidth;
            canvas.height = video.videoHeight;
            canvas.getContext("2d").drawImage(video, 0, 0);

            canvas.toBlob(
                (blob) => {
                    const file = new File([blob], "capture.jpg", { type: "image/jpeg" });
                    submitFormWithFile(file);
                },
                "image/jpeg",
                0.92,
            );
        });

        // Handle Upload Button
        if (uploadBtn) {
            uploadBtn.addEventListener("click", () => {
                fileInput.click();
            });
        }
    } catch (err) {
        console.warn("Camera access denied or failed:", err);
        // Stays in no-camera mode (form visible)
    }
}

// Form: Simple Image Preview for file selection
// This works if JS is on but camera failed/denied.
// If JS is off, this script won't run, and user gets standard file input behavior (browser dependent).
fileInput.addEventListener("change", () => {
    if (fileInput.files && fileInput.files[0]) {
        const file = fileInput.files[0];
        const url = URL.createObjectURL(file);
        imagePreview.src = url;
        imagePreview.style.display = "block";

        // If we are in camera mode, immediately submit
        if (!body.classList.contains("no-camera")) {
            submitFormWithFile(file);
        }
    }
});

form.addEventListener("submit", (event) => {
    event.preventDefault();
    submitFormWithFile();
});

export { };
