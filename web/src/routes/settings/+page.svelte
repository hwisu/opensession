<script lang="ts">
	import { goto } from '$app/navigation';
	import { SettingsPage } from '@opensession/ui/components';
</script>

<SettingsPage onNavigate={(path) => goto(path)} />
