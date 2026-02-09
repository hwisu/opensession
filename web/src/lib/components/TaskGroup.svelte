<script lang="ts">
	import { TaskGroup } from '@opensession/ui/components';
	import type { Event } from '$lib/types';

	let { events, taskId }: { events: Event[]; taskId: string } = $props();
</script>

<TaskGroup {events} {taskId} />
