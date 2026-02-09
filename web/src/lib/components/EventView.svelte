<script lang="ts">
	import { EventView } from '@opensession/ui/components';
	import type { Event } from '$lib/types';

	let { event }: { event: Event } = $props();
</script>

<EventView {event} />
