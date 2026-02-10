-- Add token tracking columns to sessions
ALTER TABLE sessions ADD COLUMN total_input_tokens INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sessions ADD COLUMN total_output_tokens INTEGER NOT NULL DEFAULT 0;

-- Add public flag to teams
ALTER TABLE teams ADD COLUMN is_public BOOLEAN NOT NULL DEFAULT 0;
