<script lang="ts">
  export let mood: 'awake' | 'prompt' | 'asleep' | 'error' = 'awake';
  export let size = 26;
  export let muted = false;
  export let animate = true;
</script>

<svg class="cat" class:muted class:animate class:awake={mood === 'awake'} class:prompt={mood === 'prompt'} width={size / 13 * 16} height={size} viewBox="0 0 16 13" shape-rendering="crispEdges" role="img" aria-label={`catabase · ${mood}`}>
  <path fill="currentColor" d="M0 0h1v1h1v1h1v1h10V2h1V1h1V0h1v10h-1v2h-2v1H3v-1H1v-2H0Z" />
  <g class="eyes" fill="var(--surface-1)">
    {#if mood === 'error'}
      <path d="M3 6h1v1h1V6h1v1H5v1h1v1H5V8H4v1H3V8h1V7H3Zm7 0h1v1h1V6h1v1h-1v1h1v1h-1V8h-1v1h-1V8h1V7h-1Z" />
    {:else if mood === 'asleep'}
      <path d="M3 8h3v1H3Zm7 0h3v1h-3Z" />
    {:else if mood === 'prompt'}
      <path d="M3 6h1v1h1v1H4v1H3V8h1V7H3Z" /><path class="cursor" d="M10 8h3v1h-3Z" />
    {:else}
      <path class="open-eyes" d="M3 7h2v2H3Zm8 0h2v2h-2Z" />
      <path class="blink-eyes" d="M3 8h2v1H3Zm8 0h2v1h-2Z" />
    {/if}
  </g>
</svg>

<style>
  .cat { display: block; flex-shrink: 0; color: var(--brand); }
  .muted { color: var(--ink-faint); }
  .blink-eyes { opacity: 0; }
  .animate.awake .open-eyes { animation: blink-open 9s steps(1) infinite; }
  .animate.awake .blink-eyes { animation: blink-closed 9s steps(1) infinite; }
  .animate.prompt .cursor { animation: cursor 1.06s steps(1) infinite; }
  @keyframes blink-open { 0%, 94%, 98% { opacity: 1; } 92%, 96% { opacity: 0; } }
  @keyframes blink-closed { 0%, 94%, 98% { opacity: 0; } 92%, 96% { opacity: 1; } }
  @keyframes cursor { 50% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .animate .open-eyes, .animate .blink-eyes, .animate .cursor { animation: none; } }
</style>
