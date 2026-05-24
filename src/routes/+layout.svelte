<script lang="ts">
  import "$lib/design/theme.css";

  import type { Snippet } from "svelte";

  let { children }: { children: Snippet } = $props();
</script>

{@render children()}
