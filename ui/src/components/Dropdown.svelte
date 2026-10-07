<script context="module" lang="ts">
  let nextDropdownId = 0;
</script>

<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';

  type DropdownOption = { value: string; label: string; disabled?: boolean; danger?: boolean };

  export let value: string;
  export let options: DropdownOption[];
  export let ariaLabel: string;
  export let disabled = false;
  export let compact = false;
  export let menuMode = false;
  export let iconOnly = false;
  export let triggerLabel = '';
  export let onSelect: (value: string) => void = () => {};

  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let menu: HTMLDivElement | undefined;
  let open = false;
  let activeIndex = 0;
  let typeahead = '';
  let lastTypedAt = 0;
  let pointerAnchor: { x: number; y: number } | null = null;
  const menuId = `dropdown-${++nextDropdownId}`;

  $: selectedIndex = options.findIndex((option) => option.value === value);
  $: selectedLabel = options[selectedIndex]?.label ?? (value || 'Choose…');
  $: unavailable = !menuMode && value !== '' && selectedIndex < 0;
  $: label = menuMode ? triggerLabel : selectedLabel;
  $: if (open && (disabled || !options.some((option) => !option.disabled))) closeMenu();
  $: if (open) void reconcileOptions(options);

  onMount(() => {
    function handleOutsidePointer(event: PointerEvent) {
      if (open && !root.contains(event.target as Node)) closeMenu();
    }

    function handleViewportChange(event?: Event) {
      // Scrolling the list must not reset its own scroll position.
      if (event?.type === 'scroll' && event.target instanceof Node && menu?.contains(event.target)) return;
      if (open) updateMenuPosition();
    }

    function handleFocusChange(event: FocusEvent) {
      if (open && !root.contains(event.target as Node)) closeMenu();
    }

    const observer = new ResizeObserver(() => handleViewportChange());
    observer.observe(trigger);
    window.addEventListener('pointerdown', handleOutsidePointer);
    window.addEventListener('focusin', handleFocusChange);
    window.addEventListener('resize', handleViewportChange);
    window.addEventListener('scroll', handleViewportChange, true);
    window.visualViewport?.addEventListener('resize', handleViewportChange);

    return () => {
      observer.disconnect();
      window.removeEventListener('pointerdown', handleOutsidePointer);
      window.removeEventListener('focusin', handleFocusChange);
      window.removeEventListener('resize', handleViewportChange);
      window.removeEventListener('scroll', handleViewportChange, true);
      window.visualViewport?.removeEventListener('resize', handleViewportChange);
    };
  });

  export async function openAt(x?: number, y?: number) {
    const anchor = typeof x === 'number' && Number.isFinite(x) && typeof y === 'number' && Number.isFinite(y)
      ? { x, y }
      : null;
    await openMenu(false, anchor);
  }

  async function openMenu(fromEnd = false, anchor: { x: number; y: number } | null = null) {
    if (disabled || !options.some((option) => !option.disabled)) return;
    pointerAnchor = anchor;
    const enabled = enabledIndexes();
    activeIndex = !menuMode && selectedIndex >= 0 && !options[selectedIndex].disabled
      ? selectedIndex
      : enabled[fromEnd ? enabled.length - 1 : 0];
    typeahead = '';
    open = true;
    await tick();
    if (!open) return;
    updateMenuPosition();
    focusActiveOption();
  }

  function closeMenu(returnFocus = false) {
    open = false;
    pointerAnchor = null;
    typeahead = '';
    if (returnFocus) trigger.focus();
  }

  function toggleMenu() {
    if (open) closeMenu();
    else void openMenu();
  }

  function choose(nextValue: string) {
    if (disabled || options.find((option) => option.value === nextValue)?.disabled) return;
    closeMenu(true);
    onSelect(nextValue);
  }

  function updateMenuPosition() {
    if (!trigger || !menu) return;
    const rect = trigger.getBoundingClientRect();
    const viewport = window.visualViewport;
    const viewportLeft = viewport?.offsetLeft ?? 0;
    const viewportTop = viewport?.offsetTop ?? 0;
    const viewportWidth = viewport?.width ?? window.innerWidth;
    const viewportHeight = viewport?.height ?? window.innerHeight;
    const padding = 8;
    const gap = pointerAnchor ? 2 : 6;
    const anchorX = pointerAnchor
      ? Math.max(viewportLeft + padding, Math.min(pointerAnchor.x, viewportLeft + viewportWidth - padding))
      : rect.left;
    const anchorTop = pointerAnchor
      ? Math.max(viewportTop + padding, Math.min(pointerAnchor.y, viewportTop + viewportHeight - padding))
      : rect.top;
    const anchorBottom = pointerAnchor ? anchorTop : rect.bottom;
    const availableWidth = Math.max(0, viewportWidth - padding * 2);
    const previousScroll = menu.scrollTop;

    // Measure the rendered, wrapped content before choosing a side. Long model
    // names can make options taller than the usual one-line menu row.
    Object.assign(menu.style, {
      width: 'max-content',
      minWidth: `${Math.min(Math.max(pointerAnchor ? 0 : rect.width, 180), availableWidth)}px`,
      maxWidth: `${Math.min(460, availableWidth)}px`,
      maxHeight: 'none'
    });
    const naturalHeight = menu.getBoundingClientRect().height;
    const below = Math.max(0, viewportTop + viewportHeight - anchorBottom - gap - padding);
    const above = Math.max(0, anchorTop - viewportTop - gap - padding);
    const opensUp = below < Math.min(naturalHeight, 320) && above > below;
    menu.style.maxHeight = `${Math.min(320, opensUp ? above : below)}px`;
    const measured = menu.getBoundingClientRect();
    const preferredLeft = pointerAnchor ? anchorX : menuMode ? rect.right - measured.width : rect.left;
    const left = Math.max(viewportLeft + padding, Math.min(preferredLeft, viewportLeft + viewportWidth - measured.width - padding));
    const preferredTop = opensUp ? anchorTop - measured.height - gap : anchorBottom + gap;
    const top = Math.max(viewportTop + padding, Math.min(preferredTop, viewportTop + viewportHeight - measured.height - padding));
    Object.assign(menu.style, { top: `${top}px`, left: `${left}px`, visibility: 'visible' });
    menu.scrollTop = previousScroll;
  }

  function focusActiveOption() {
    const option = menu?.querySelector<HTMLButtonElement>(`[data-dropdown-index="${activeIndex}"]`);
    if (!menu || !option) return;
    option.focus({ preventScroll: true });
    const optionBounds = option.getBoundingClientRect();
    const menuTop = menu.getBoundingClientRect().top + menu.clientTop;
    const menuBottom = menuTop + menu.clientHeight;
    // Scroll only this fixed menu; scrollIntoView can also move its sidebar.
    if (optionBounds.top < menuTop) menu.scrollTop += optionBounds.top - menuTop;
    else if (optionBounds.bottom > menuBottom) menu.scrollTop += optionBounds.bottom - menuBottom;
  }

  function enabledIndexes() {
    return options.flatMap((option, index) => option.disabled ? [] : [index]);
  }

  async function reconcileOptions(nextOptions: DropdownOption[]) {
    const focusedOption = menu?.contains(document.activeElement)
      ? document.activeElement as HTMLButtonElement
      : undefined;
    const focusedIndex = focusedOption
      ? nextOptions.findIndex((option) => option.value === focusedOption.dataset.dropdownValue && !option.disabled)
      : -1;
    if (focusedIndex >= 0) {
      activeIndex = focusedIndex;
    } else if (focusedOption || !nextOptions[activeIndex] || nextOptions[activeIndex].disabled) {
      activeIndex = nextOptions.findIndex((option) => !option.disabled);
    }
    await tick();
    if (open) {
      updateMenuPosition();
      // Async model discovery may replace/remove the button that had focus.
      if (focusedOption) focusActiveOption();
    }
  }

  function moveActive(delta: number) {
    const enabled = enabledIndexes();
    activeIndex = enabled[(enabled.indexOf(activeIndex) + delta + enabled.length) % enabled.length];
    focusActiveOption();
  }

  function matchTypedCharacter(character: string) {
    const now = performance.now();
    typeahead = now - lastTypedAt < 700 ? typeahead + character.toLocaleLowerCase() : character.toLocaleLowerCase();
    lastTypedAt = now;
    // Repeating a letter cycles through matching options.
    const search = Array.from(typeahead).every((letter) => letter === typeahead[0]) ? typeahead[0] : typeahead;
    const indexes = enabledIndexes();
    const start = search.length > 1 ? indexes.indexOf(activeIndex) : indexes.indexOf(activeIndex) + 1;
    for (let offset = 0; offset < indexes.length; offset += 1) {
      const index = indexes[(Math.max(0, start) + offset) % indexes.length];
      if (options[index].label.toLocaleLowerCase().startsWith(search)) {
        activeIndex = index;
        focusActiveOption();
        return;
      }
    }
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing) return;
    if (menuMode && iconOnly && (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))) {
      event.preventDefault();
      event.stopPropagation();
      void openAt();
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      event.stopPropagation();
      void openMenu(event.key === 'ArrowUp');
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      event.stopPropagation();
      toggleMenu();
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      event.stopPropagation();
      closeMenu();
    } else if (event.key.length === 1) {
      event.preventDefault();
      void openMenu().then(() => { if (open) matchTypedCharacter(event.key); });
    }
  }

  function handleTriggerContextMenu(event: MouseEvent) {
    if (!menuMode || !iconOnly) return;
    event.preventDefault();
    event.stopPropagation();
    void openAt(event.clientX, event.clientY);
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    // Modified shortcuts (including Cmd/Ctrl+Enter) belong to the app.
    if (event.metaKey || event.ctrlKey || event.altKey || event.isComposing) return;
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      event.stopPropagation();
      moveActive(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      event.stopPropagation();
      const enabled = enabledIndexes();
      activeIndex = enabled[event.key === 'Home' ? 0 : enabled.length - 1];
      focusActiveOption();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      closeMenu(true);
    } else if (event.key === 'Tab') {
      // Return to the trigger before native tab navigation so removing the
      // focused option does not send focus back to the beginning of the page.
      closeMenu(true);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      event.stopPropagation();
      const option = options[activeIndex];
      if (option) choose(option.value);
    } else if (event.key.length === 1) {
      event.preventDefault();
      event.stopPropagation();
      matchTypedCharacter(event.key);
    }
  }
</script>

<div bind:this={root} class="dropdown" class:compact class:open class:action-menu={menuMode} class:icon-only={iconOnly}>
  <button
    bind:this={trigger}
    type="button"
    class="dropdown-trigger"
    aria-label={menuMode ? ariaLabel : `${ariaLabel}: ${label}${unavailable ? ' (unavailable)' : ''}`}
    title={iconOnly ? ariaLabel : unavailable ? `${label} — unavailable; choose another option` : label}
    aria-haspopup={menuMode ? 'menu' : 'listbox'}
    aria-controls={open ? menuId : undefined}
    aria-expanded={open}
    {disabled}
    on:click={toggleMenu}
    on:keydown={handleTriggerKeydown}
    on:contextmenu={handleTriggerContextMenu}
  >
    {#if iconOnly}
      <Icon name="ellipsis" size={18} />
    {:else}
      <span class="dropdown-label">{label}</span>
      {#if unavailable}<span class="unavailable-hint">Unavailable</span>{/if}
      <span class="dropdown-chevron"><Icon name="chevron" size={14} /></span>
    {/if}
  </button>

  {#if open}
    <div bind:this={menu} id={menuId} class="dropdown-menu" role={menuMode ? 'menu' : 'listbox'} aria-label={ariaLabel} tabindex="-1" style="visibility:hidden" on:keydown={handleMenuKeydown}>
      {#each options as option, index (option.value)}
        <button
          type="button"
          class="dropdown-option"
          class:active={index === activeIndex}
          class:danger={option.danger}
          role={menuMode ? 'menuitem' : 'option'}
          aria-selected={menuMode ? undefined : option.value === value}
          disabled={option.disabled}
          tabindex="-1"
          data-dropdown-index={index}
          data-dropdown-value={option.value}
          on:focus={() => (activeIndex = index)}
          on:click={() => choose(option.value)}
        >
          <span>{option.label}</span>
          {#if !menuMode && option.value === value}<Icon name="check" size={14} />{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .dropdown-menu {
    box-sizing: border-box;
    overscroll-behavior: contain;
    grid-auto-rows: min-content;
  }

  .dropdown-option {
    flex-shrink: 0;
    gap: 1rem;
  }

  .dropdown-option span {
    min-width: 0;
    white-space: normal;
    overflow-wrap: anywhere;
    line-height: 1.4;
  }

  .dropdown-option.danger {
    color: #fda4af;
  }

  .dropdown-option:disabled {
    opacity: 0.5;
  }

  .unavailable-hint {
    flex: 0 0 auto;
    color: #fbbf24;
    font-size: 0.7rem;
  }

  .action-menu {
    width: auto;
    flex: 0 0 auto;
  }

  .action-menu .dropdown-trigger {
    min-width: 0;
  }

  .dropdown.icon-only .dropdown-trigger {
    width: 32px;
    min-width: 32px;
    height: 32px;
    justify-content: center;
    padding: 0;
  }
</style>
