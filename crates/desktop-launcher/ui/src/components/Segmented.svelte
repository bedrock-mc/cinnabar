<script lang="ts" generics="T extends string">
  interface Props {
    label: string;
    options: { value: T; label: string }[];
    value: T;
    disabled?: boolean;
    onchange: (value: T) => void;
  }

  let { label, options, value, disabled = false, onchange }: Props = $props();
  const index = $derived(Math.max(0, options.findIndex((o) => o.value === value)));

  // Arrow keys move the selection, as in a native segmented control.
  function onKey(e: KeyboardEvent) {
    const step = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const next = options[(index + step + options.length) % options.length]!;
    onchange(next.value);
    (e.currentTarget as HTMLElement).querySelector<HTMLElement>(`[data-value="${next.value}"]`)?.focus();
  }
</script>

<div class="segmented" role="radiogroup" aria-label={label} style:--n={options.length} style:--i={index} onkeydown={onKey} tabindex="-1">
  <span class="thumb" aria-hidden="true"></span>
  {#each options as option (option.value)}
    <button
      role="radio"
      data-value={option.value}
      aria-checked={option.value === value}
      tabindex={option.value === value ? 0 : -1}
      {disabled}
      onclick={() => onchange(option.value)}>{option.label}</button
    >
  {/each}
</div>

<style>
  .segmented {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    padding: 3px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.28);
    box-shadow: inset 0 0 0 1px var(--glass-edge);
    outline: none;
  }

  .thumb {
    position: absolute;
    top: 3px;
    bottom: 3px;
    left: 3px;
    width: calc((100% - 6px) / var(--n));
    border-radius: 999px;
    background: var(--ink);
    transform: translateX(calc(var(--i) * 100%));
    transition: transform var(--t-med) var(--ease-out);
  }

  button {
    position: relative;
    height: 32px;
    padding: 0 18px;
    border-radius: 999px;
    font: var(--t-label);
    color: var(--ink-2);
    transition: color var(--t-fast) linear;
  }

  button:hover:not([aria-checked='true']):not(:disabled) {
    color: var(--ink);
  }

  button[aria-checked='true'] {
    color: var(--solid);
  }

  button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  @media (prefers-reduced-motion: reduce) {
    .thumb {
      transition: none;
    }
  }
</style>
