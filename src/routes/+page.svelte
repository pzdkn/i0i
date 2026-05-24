<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let message = $state("Click the button to ask Rust for data.");
  let isLoading = $state(false);

  async function askRust() {
    isLoading = true;

    try {
      message = await invoke<string>("get_app_message");
    } catch (error) {
      message = `Rust command failed: ${String(error)}`;
    } finally {
      isLoading = false;
    }
  }
</script>

<main>
  <h1>Tauri Bridge Foundation</h1>
  <button type="button" onclick={askRust} disabled={isLoading}>
    {isLoading ? "Calling Rust..." : "Ask Rust"}
  </button>
  <p>{message}</p>
</main>

<style>
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

main {
  min-height: 100vh;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 1rem;
  text-align: center;
  padding: 2rem;
}

h1 {
  margin: 0;
  font-size: 2rem;
}

button {
  border-radius: 8px;
  border: 1px solid #396cd8;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #ffffff;
  background-color: #396cd8;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
  cursor: pointer;
}

button:hover {
  border-color: #2f5fbd;
  background-color: #2f5fbd;
}

button:active {
  border-color: #244a95;
  background-color: #244a95;
}

button {
  outline: none;
}

button:disabled {
  cursor: wait;
  opacity: 0.7;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  button {
    border-color: #24c8db;
    background-color: #24c8db;
    color: #0f0f0f;
  }
}
</style>
