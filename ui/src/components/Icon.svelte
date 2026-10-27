<script lang="ts">
  type IconName =
    | 'check'
    | 'chevron'
    | 'clear'
    | 'close'
    | 'copy'
    | 'database'
    | 'download'
    | 'edit'
    | 'ellipsis'
    | 'history'
    | 'format'
    | 'explain'
    | 'key'
    | 'link'
    | 'open'
    | 'plus'
    | 'play'
    | 'refresh'
    | 'schema'
    | 'search'
    | 'settings'
    | 'sparkles'
    | 'stop'
    | 'tabs'
    | 'trash';

  export let name: IconName;
  export let size = 18;
</script>

<svg
  aria-hidden="true"
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.75"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  {#if name === 'format'}
    <path d="m4 7-3 5 3 5M20 7l3 5-3 5M14 4l-4 16M7 8h3M6 12h3M5 16h3" />
  {:else if name === 'explain'}
    <rect x="9" y="3" width="6" height="5" rx="1" /><path d="M12 8v4M5 16v-4h14v4" /><rect x="2" y="16" width="6" height="5" rx="1" /><rect x="16" y="16" width="6" height="5" rx="1" />
  {:else if name === 'database'}
    <ellipse cx="12" cy="5" rx="7" ry="3" />
    <path d="M5 5v7c0 1.7 3.1 3 7 3s7-1.3 7-3V5" />
    <path d="M5 12v7c0 1.7 3.1 3 7 3s7-1.3 7-3v-7" />
  {:else if name === 'schema'}
    <rect x="3" y="3" width="7" height="6" rx="1.5" />
    <rect x="14" y="15" width="7" height="6" rx="1.5" />
    <rect x="3" y="15" width="7" height="6" rx="1.5" />
    <path d="M6.5 9v3h11v3M6.5 12v3" />
  {:else if name === 'tabs'}
    <rect x="3" y="6" width="18" height="15" rx="1.5" /><path d="M6 3h12M3 11h18M9 6v5" />
  {:else if name === 'history'}
    <path d="M3 12a9 9 0 1 0 3-6.7L3 8" />
    <path d="M3 3v5h5M12 7v5l3 2" />
  {:else if name === 'key'}
    <circle cx="8" cy="8" r="5" />
    <path d="m11.5 11.5 9 9M17 17l3-3M14 14l3-3" />
  {:else if name === 'link'}
    <path d="m10 13 4-4M8 15l-1 1a4 4 0 0 1-6-6l4-4a4 4 0 0 1 6 0M13 9l1-1a4 4 0 0 1 6 6l-4 4a4 4 0 0 1-6 0" />
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.88l.06.06-2.83 2.83-.06-.06A1.7 1.7 0 0 0 15 19.4a1.7 1.7 0 0 0-1 .6 1.7 1.7 0 0 0-.4 1.1V21h-4v-.1A1.7 1.7 0 0 0 8.6 19.4a1.7 1.7 0 0 0-1.88.34l-.06.06-2.83-2.83.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-.6-1 1.7 1.7 0 0 0-1.1-.4H3v-4h.1A1.7 1.7 0 0 0 4.6 8.6a1.7 1.7 0 0 0-.34-1.88l-.06-.06 2.83-2.83.06.06A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-.6 1.7 1.7 0 0 0 .4-1.1V3h4v.1A1.7 1.7 0 0 0 15.4 4.6a1.7 1.7 0 0 0 1.88-.34l.06-.06 2.83 2.83-.06.06A1.7 1.7 0 0 0 19.4 9c.14.36.36.7.6 1 .28.3.66.46 1.1.5h.1v4h-.1c-.44.04-.82.2-1.1.5-.24.3-.46.64-.6 1Z" />
  {:else if name === 'play'}
    <path d="m8 5 11 7-11 7V5Z" />
  {:else if name === 'stop'}
    <rect x="6" y="6" width="12" height="12" rx="2" />
  {:else if name === 'clear'}
    <path d="m4 16 9-9 4 4-9 9H4v-4Z" />
    <path d="m11 9 4 4M13 20h7" />
  {:else if name === 'plus'}
    <path d="M12 5v14M5 12h14" />
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="7" />
    <path d="m20 20-4-4" />
  {:else if name === 'refresh'}
    <path d="M20 7v5h-5M4 17v-5h5" />
    <path d="M6.1 9a7 7 0 0 1 11.7-2L20 12M4 12l2.2 5a7 7 0 0 0 11.7-2" />
  {:else if name === 'check'}
    <path d="m5 12 4 4L19 6" />
  {:else if name === 'edit'}
    <path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4L16.5 3.5Z" />
  {:else if name === 'ellipsis'}
    <circle cx="5" cy="12" r="1.5" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="1.5" fill="currentColor" stroke="none" />
    <circle cx="19" cy="12" r="1.5" fill="currentColor" stroke="none" />
  {:else if name === 'trash'}
    <path d="M4 7h16M9 7V4h6v3M7 7l1 14h8l1-14M10 11v6M14 11v6" />
  {:else if name === 'copy'}
    <rect x="8" y="8" width="11" height="11" rx="2" />
    <path d="M16 8V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h3" />
  {:else if name === 'open'}
    <path d="M14 3h7v7M21 3 10 14M10 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-5" />
  {:else if name === 'download'}
    <path d="M12 3v12M7 10l5 5 5-5M5 21h14" />
  {:else if name === 'chevron'}
    <path d="m9 6 6 6-6 6" />
  {:else if name === 'sparkles'}
    <path d="m12 3 1.2 3.8L17 8l-3.8 1.2L12 13l-1.2-3.8L7 8l3.8-1.2L12 3ZM19 14l.7 2.3L22 17l-2.3.7L19 20l-.7-2.3L16 17l2.3-.7L19 14ZM5 13l.8 2.2L8 16l-2.2.8L5 19l-.8-2.2L2 16l2.2-.8L5 13Z" />
  {:else if name === 'close'}
    <path d="m6 6 12 12M18 6 6 18" />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex: 0 0 auto;
  }
</style>
