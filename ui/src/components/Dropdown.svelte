<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon from './Icon.svelte';

  type DropdownOption = { value: string; label: string };

  export let value: string;
  export let options: DropdownOption[];
  export let ariaLabel: string;
  export let disabled = false;
  export let compact = false;
  export let onSelect: (value: string) => void = () => {};

  let root: HTMLDivElement;
  let trigger: HTMLButtonElement;
  let open = false;
  let activeIndex = 0;
  let menuStyle = '';

  $: selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  $: selectedLabel = options[selectedIndex]?.label ?? value;

  onMount(() => {
    function handleOutsidePointer(event: PointerEvent) {
      if (open && !root.contains(event.target as Node)) closeMenu();
    }

    function handleViewportChange() {
      if (open) updateMenuPosition();
    }

    window.addEventListener('pointerdown', handleOutsidePointer);
    window.addEventListener('resize', handleViewportChange);
    window.addEventListener('scroll', handleViewportChange, true);

    return () => {
      window.removeEventListener('pointerdown', handleOutsidePointer);
      window.removeEventListener('resize', handleViewportChange);
      window.removeEventListener('scroll', handleViewportChange, true);
    };
  });

  async function openMenu(direction = 0) {
    if (disabled || options.length === 0) return;
    activeIndex = Math.min(Math.max(selectedIndex + direction, 0), options.length - 1);
    open = true;
    updateMenuPosition();
    await tick();
    focusActiveOption();
  }

  function closeMenu(returnFocus = false) {
    open = false;
    if (returnFocus) trigger.focus();
  }

  function toggleMenu() {
    if (open) closeMenu();
    else void openMenu();
  }

  function choose(nextValue: string) {
    onSelect(nextValue);
    closeMenu(true);
  }

  function updateMenuPosition() {
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const estimatedHeight = Math.min(options.length * 36 + 10, 240);
    const roomBelow = window.innerHeight - rect.bottom;
    const opensUp = roomBelow < Math.min(estimatedHeight + 8, 160) && rect.top > roomBelow;
    const top = opensUp ? Math.max(8, rect.top - estimatedHeight - 6) : rect.bottom + 6;
    const left = Math.min(rect.left, window.innerWidth - rect.width - 8);
    menuStyle = `top:${Math.round(top)}px;left:${Math.max(8, Math.round(left))}px;width:${Math.round(rect.width)}px;max-height:240px`;
  }

  function focusActiveOption() {
    root.querySelector<HTMLButtonElement>(`[data-dropdown-index="${activeIndex}"]`)?.focus();
  }

  function moveActive(delta: number) {
    activeIndex = (activeIndex + delta + options.length) % options.length;
    void tick().then(focusActiveOption);
  }

  function handleTriggerKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      void openMenu(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      toggleMenu();
    } else if (event.key === 'Escape' && open) {
      event.preventDefault();
      closeMenu();
    }
  }

  function handleMenuKeydown(event: KeyboardEvent) {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      moveActive(event.key === 'ArrowDown' ? 1 : -1);
    } else if (event.key === 'Home' || event.key === 'End') {
      event.preventDefault();
      activeIndex = event.key === 'Home' ? 0 : options.length - 1;
      void tick().then(focusActiveOption);
    } else if (event.key === 'Escape' || event.key === 'Tab') {
      closeMenu(event.key === 'Escape');
    }
  }
</script>

<div bind:this={root} class="dropdown" class:compact class:open>
  <button
    bind:this={trigger}
    type="button"
    class="dropdown-trigger"
    aria-label={ariaLabel}
    aria-haspopup="listbox"
    aria-expanded={open}
    {disabled}
    on:click={toggleMenu}
    on:keydown={handleTriggerKeydown}
  >
    <span class="dropdown-label">{selectedLabel}</span>
    <span class="dropdown-chevron"><Icon name="chevron" size={14} /></span>
  </button>

  {#if open}
    <div class="dropdown-menu" role="listbox" aria-label={ariaLabel} tabindex="-1" style={menuStyle} on:keydown={handleMenuKeydown}>
      {#each options as option, index (option.value)}
        <button
          type="button"
          class="dropdown-option"
          class:active={index === activeIndex}
          role="option"
          aria-selected={option.value === value}
          data-dropdown-index={index}
          on:mouseenter={() => (activeIndex = index)}
          on:click={() => choose(option.value)}
        >
          <span>{option.label}</span>
          {#if option.value === value}<Icon name="check" size={14} />{/if}
        </button>
      {/each}
    </div>
  {/if}
</div>
