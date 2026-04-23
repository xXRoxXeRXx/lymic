<script lang="ts">
	import { cn } from "$lib/utils.js";
	import type { HTMLAttributes } from "svelte/elements";

	let { class: className, children, ...rest }: HTMLAttributes<HTMLDivElement> = $props();
</script>

<div
	class={cn("rounded-lg border bg-zinc-900 border-zinc-800 text-zinc-100 shadow-sm", className)}
	{...rest}
>
	{@render children?.()}
</div>
