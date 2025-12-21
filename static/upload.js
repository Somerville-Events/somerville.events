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

let stream = null;

if (!cameraUi || !video || !shutterBtn || !uploadBtn || !canvas || !skeleton || !form || !fileInput || !imagePreview) {
    throw new Error("One or more elements not found");
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
            shutterBtn.disabled = true;
            if (uploadBtn) uploadBtn.disabled = true;

            // Freeze video to show what was captured
            video.pause();

            canvas.width = video.videoWidth;
            canvas.height = video.videoHeight;
            canvas.getContext("2d").drawImage(video, 0, 0);

            canvas.toBlob((blob) => {
                // Update File Input
                const file = new File([blob], "capture.jpg", { type: "image/jpeg" });
                const dataTransfer = new DataTransfer();
                dataTransfer.items.add(file);
                fileInput.files = dataTransfer.files;

                // Submit immediately
                form.submit();
            }, "image/jpeg");
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
        // If we are in camera mode, immediately submit
        if (!body.classList.contains("no-camera")) {
            if (uploadBtn) {
                uploadBtn.innerHTML = '<span class="spinner"></span> Uploading...';
                uploadBtn.disabled = true;
            }
            shutterBtn.disabled = true;
            form.submit();
            return;
        }

        const file = fileInput.files[0];
        const url = URL.createObjectURL(file);
        imagePreview.src = url;
        imagePreview.style.display = "block";

        // Update label to show filename (optional but helpful feedback)
        // const label = document.querySelector('label[for="image"]');
        // if (label) label.textContent = "Change File (" + file.name + ")";
    }
});

// Handle form submit state
form.addEventListener("submit", function () {
    const btn = form.querySelector('button[type="submit"]');
    btn.style.opacity = "0.8";
    btn.innerHTML = "Uploading...";
});

export { }