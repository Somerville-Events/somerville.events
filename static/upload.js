const form = document.querySelector("form");
const fileInput = document.querySelector('input[type="file"]');
const imagePreview = document.querySelector("form img");

if (!form || !fileInput || !imagePreview) {
    throw new Error("One or more elements not found");
}


// Simple Image Preview for file selection
// If JS is off, this script won't run, and user gets
// the standard file input behavior (browser dependent).
fileInput.addEventListener("change", () => {
    if (fileInput.files && fileInput.files[0]) {
        const file = fileInput.files[0];
        const url = URL.createObjectURL(file);
        imagePreview.src = url;
        imagePreview.style.display = "block";
    }
});

// Handle form submit state
form.addEventListener("submit", function () {
    const btn = form.querySelector('button[type="submit"]');
    btn.style.opacity = "0.8";
    btn.innerHTML = "Uploading...";
});

export { }