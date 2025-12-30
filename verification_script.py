from playwright.sync_api import Page, expect, sync_playwright

def verify_frontend(page: Page):
    # 1. Navigate to index
    page.goto("http://localhost:8080/")

    # 2. Check for event in list
    expect(page.get_by_text("Test Event 1")).to_be_visible()

    # 3. Screenshot index
    page.screenshot(path="/home/jules/verification/index.png")

    # 4. Navigate to detail
    page.get_by_text("Test Event 1").click()

    # 5. Check for new fields
    expect(page.get_by_text("Categories")).to_be_visible()
    expect(page.get_by_text("Music")).to_be_visible()
    expect(page.get_by_text("Child Friendly")).to_be_visible()

    expect(page.get_by_text("Ages")).to_be_visible()
    expect(page.get_by_text("21+")).to_be_visible()

    expect(page.get_by_text("Price")).to_be_visible()
    expect(page.get_by_text("$15")).to_be_visible()

    expect(page.get_by_text("Source")).to_be_visible()
    expect(page.get_by_text("Somerville Theatre")).to_be_visible()

    # 6. Screenshot detail
    page.screenshot(path="/home/jules/verification/detail.png")

if __name__ == "__main__":
    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page()
        try:
            verify_frontend(page)
            print("Verification successful!")
        except Exception as e:
            print(f"Verification failed: {e}")
            page.screenshot(path="/home/jules/verification/failure.png")
        finally:
            browser.close()
