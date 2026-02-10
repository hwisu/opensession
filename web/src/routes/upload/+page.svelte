<script lang="ts">
	import { UploadPage } from '@opensession/ui/components';
	import { goto } from '$app/navigation';
</script>

<UploadPage teamMode="manual" onSuccess={(id) => goto(`/session/${id}`)} />
