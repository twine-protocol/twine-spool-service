<script lang="ts">
  import { StructuredList, StructuredListHead, StructuredListBody, StructuredListRow, StructuredListSkeleton, StructuredListCell, Button, Tile, ComposedModal, ModalHeader, ModalBody, Form, ModalFooter, TextInput, DatePicker, DatePickerInput } from 'carbon-components-svelte'
  import * as ApiKeys from '$lib/state/ApiKeys'
	import { Add } from 'carbon-icons-svelte'

  let createDialog = $state(false)

  let apiKeys = $state(ApiKeys.list())

  const defaultData = () => ({
    description: '',
    expires_at: ''
  })

  let keyData = $state(defaultData())

  const cancelCreate = () => {
    createDialog = false
    keyData = defaultData()
  }

  async function submit() {
    try {
      await ApiKeys.create({ description: keyData.description, expires_at: keyData.expires_at })
      cancelCreate()
      apiKeys = ApiKeys.list()
    } catch (error) {
      console.error('Error creating API key:', error)
    }
  }


  async function remove(id: number){
    try {
      await ApiKeys.remove(id)
      cancelCreate()
      apiKeys = ApiKeys.list()
    } catch (error) {
      console.error('Error creating API key:', error)
    }
  }

</script>

{#await apiKeys}
  <StructuredListSkeleton />
{:then apiKeys}
    <StructuredList>
      <StructuredListHead>
        <StructuredListRow head>
          <StructuredListCell head>Description</StructuredListCell>
          <StructuredListCell head>Created</StructuredListCell>
          <StructuredListCell head>Last Used</StructuredListCell>
          <StructuredListCell head>Expires</StructuredListCell>
          <StructuredListCell head>Actions</StructuredListCell>
        </StructuredListRow>
      </StructuredListHead>
      <StructuredListBody>
        {#each apiKeys as key}
        <StructuredListRow>
          <StructuredListCell>{key.description}</StructuredListCell>
          <StructuredListCell>{key.created_at}</StructuredListCell>
          <StructuredListCell>{key.last_used_at}</StructuredListCell>
          <StructuredListCell>{key.expires_at}</StructuredListCell>
          <StructuredListCell>
            <Button kind="danger" on:click={() => remove(key.id)}>Delete</Button>
          </StructuredListCell>
        </StructuredListRow>
        {/each}
      </StructuredListBody>
    </StructuredList>

{:catch error}
  <p>Error loading API keys: {error.message}</p>
{/await}

<Tile>
  <Button icon={Add} kind="primary" on:click={() => createDialog = true}>
    Create API Key
  </Button>
</Tile>

<ComposedModal bind:open={createDialog} on:submit={submit} on:close={cancelCreate} containerClass="modal-container-apikey">
  <ModalHeader label="Create API Key" />
  <ModalBody hasScrollingContent>
    <Form>
      <TextInput labelText="Description" bind:value={keyData.description} placeholder="Description" />
      <DatePicker datePickerType="single" dateFormat="Z" bind:value={keyData.expires_at}>
        <DatePickerInput labelText="Expiration Date" />
      </DatePicker>
    </Form>
  </ModalBody>
  <ModalFooter primaryButtonText="Create" secondaryButtonText="Cancel" />
</ComposedModal>

<style>
  :global(.modal-container-apikey .bx--modal-content) {
    min-height: 500px;
  }
</style>