<script lang="ts">
  interface Props {
    /** A 64x64 skin image; the head's front face and hat layer are cropped from it. */
    skinUrl?: string;
    size: number;
  }

  let { skinUrl, size }: Props = $props();
  let canvas: HTMLCanvasElement;

  $effect(() => {
    const ctx = canvas.getContext('2d')!;
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, 8, 8);
    if (!skinUrl) return;
    const img = new Image();
    img.onload = () => {
      ctx.clearRect(0, 0, 8, 8);
      ctx.drawImage(img, 8, 8, 8, 8, 0, 0, 8, 8);
      ctx.drawImage(img, 40, 8, 8, 8, 0, 0, 8, 8);
    };
    img.src = skinUrl;
  });
</script>

<canvas bind:this={canvas} width="8" height="8" style:width="{size}px" style:height="{size}px" aria-hidden="true"></canvas>

<style>
  canvas {
    display: block;
    image-rendering: pixelated;
    border-radius: 6px;
    background: var(--raise-2);
  }
</style>
