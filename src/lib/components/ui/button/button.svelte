<script lang="ts">
	import { buttonVariants, type Props } from "./index.js";
	import { cn } from "$lib/utils.js";

	let {
		class: className,
		variant = "default",
		size = "default",
		children,
		...rest
	}: Props = $props();
</script>

<button
	class={cn(buttonVariants({ variant, size }), className)}
	{...rest}
>
	{@render children?.()}
</button>
