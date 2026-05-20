import { SearchField } from "@heroui/react";

interface SearchBarProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
}

export function SearchBar({
  value,
  onChange,
  placeholder,
}: SearchBarProps) {
  return (
    <SearchField.Root
      value={value}
      onChange={onChange}
      className="w-full max-w-2xl mx-auto"
    >
      <SearchField.Group>
        <SearchField.SearchIcon />
        <SearchField.Input placeholder={placeholder} className="h-8" />
        {value && <SearchField.ClearButton />}
      </SearchField.Group>
    </SearchField.Root>
  );
}
